use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use axum::response::IntoResponse;
use libp2p::swarm::Swarm;
use std::time::Duration;
use axum::extract::State;
use log::error;
use axum::{
    routing::post,
    Json, Router,
};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use log::info;
use env_logger::{Env, Builder};

use libp2p::{
    Multiaddr,
    identity,
    PeerId,
    StreamProtocol,
    SwarmBuilder,
    tcp::Config as TcpConfig,
    yamux::Config as YamuxConfig
};

use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::noise::Config as NoiceConfig;

use libp2p::identify::{
    Config as IdentifyConfig,
    Behaviour as IdentifyBehavior,
    Event as IdentifyEvent,
};

use libp2p::kad::{
    Config as KadConfig,
    Behaviour as KademliaBehavior,
    store::MemoryStore as KadInMemory,
};

use libp2p::request_response::{
    Config as RequestResponseConfig,
    ProtocolSupport as RequestResponseProtocolSupport,
};
use narwhal::p2p::PeerManager;
use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;

mod behavior;
use behavior::{
    Behavior,
    Event as AgentEvent,
    BehaviorContext,
    PeerManagement,
};

mod message;
use message::TransactionMessage;
use narwhal::transaction::Transaction;
use narwhal::dag::DAG;

use serde_json::json;

use libp2p::core::ConnectedPoint;

use axum::debug_handler;

type _PeerMap = Arc<TokioMutex<HashMap<PeerId, Vec<Multiaddr>>>>;
type SharedState = (
    Arc<TokioMutex<PeerManager>>,
    Arc<TokioMutex<DAG>>,
    Arc<TokioMutex<Swarm<Behavior>>>
);

type SharedPeerManager = Arc<TokioMutex<PeerManager>>;

#[derive(Serialize, Deserialize, Debug)]
struct TransactionRequestData {
    data: String,
    parents: Vec<String>,
}

pub struct Node<T: PeerManagement> {
    swarm: Swarm<Behavior>,
    context: Arc<BehaviorContext<T>>,
}

impl<T: PeerManagement> Node<T> {
    pub fn new(swarm: Swarm<Behavior>, context: Arc<BehaviorContext<T>>) -> Self {
        Self { swarm, context }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    Builder::from_env(Env::default().default_filter_or("debug")).init();

    // Generate a local keypair
    let local_key = identity::Keypair::generate_ed25519();
    
    // Create a DAG for the current node
    let dag = Arc::new(TokioMutex::new(DAG::new()));

    // Build the Swarm
    let mut swarm = build_swarm(local_key.clone(), dag.clone()).await?;

    // Set the server mode
    swarm.behaviour_mut().set_server_mode();

    // Listen and dial based on the provided argument
    if let Some(addr) = args().nth(1) {
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        let remote: Multiaddr = addr.parse()?;
        
        // Add retry logic for initial connection
        let mut retry_count = 0;
        while retry_count < 3 {
            match swarm.dial(remote.clone()) {
                Ok(_) => {
                    info!("Dialed to: {addr}");
                    break;
                }
                Err(e) => {
                    error!("Failed to dial {}: {}", addr, e);
                    retry_count += 1;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    } else {
        info!("Acting as bootstrap node");
        swarm.listen_on("/ip4/0.0.0.0/tcp/8000".parse()?)?;
    }

    // Instead of calling handle_swarm_events directly, spawn it as a task
    let swarm = Arc::new(TokioMutex::new(swarm));
    let swarm_task = {
        let dag = dag.clone();
        let swarm_clone = swarm.clone();
        let peer_manager = Arc::new(TokioMutex::new(PeerManager::new("peers.json")));

        tokio::spawn(async move {
            loop {
                let event = {
                    let mut swarm = swarm_clone.lock().await;
                    swarm.select_next_some().await
                };

                // Create a new block to handle events
                {
                    let mut swarm = swarm_clone.lock().await;
                    if let Err(e) = handle_event(&mut swarm, &event, &peer_manager, dag.clone()).await {
                        error!("Error handling swarm event: {}", e);
                        break;
                    }
                }
            }
        })
    };

    // Create shared state
    let peer_manager = Arc::new(TokioMutex::new(PeerManager::new("peers.json")));
    let state = (
        peer_manager.clone(),
        dag.clone(),
        swarm.clone()
    );

    // Initialize the Axum server with state
    let app = Router::new()
        .route("/transaction", post(receive_transaction))
        .with_state(state);

    // In main function, modify the argument handling
    let http_port = match args().nth(2) {
        Some(port) => port,
        None => "3000".to_string()  // Default port if not specified
    };

    // Spawn the HTTP server with the specified port
    let server = tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", http_port);
        info!("Starting HTTP server on {}", addr);
        axum::serve(
            tokio::net::TcpListener::bind(&addr).await.expect("Failed to bind"),
            app.into_make_service()
        )
        .await
        .unwrap();
    });

    // Wait for both tasks
    tokio::select! {
        _ = swarm_task => println!("Swarm task completed"),
        _ = server => println!("Server task completed"),
    }

    Ok(())
}

// Function to build and configure the swarm
async fn build_swarm(local_key: identity::Keypair, _dag: Arc<TokioMutex<DAG>>) -> Result<Swarm<Behavior>, Box<dyn std::error::Error>> {
    let behavior = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            TcpConfig::default(), 
            NoiceConfig::new, 
            YamuxConfig::default
        )?
        .with_behaviour(|key| {
            let local_peer_id = PeerId::from(key.public());
            info!("LocalPeerID: {local_peer_id}");

            let kad_memory = KadInMemory::new(local_peer_id);
            let kad = KademliaBehavior::with_config(
                local_peer_id, 
                kad_memory, 
                KadConfig::default()
            );

            let identify = IdentifyBehavior::new(
                IdentifyConfig::new(
                    "/agent/connection/1.0.0".to_string(),
                    key.public(),
                )
                .with_push_listen_addr_updates(true)
                .with_interval(Duration::from_secs(30))
            );

            let rr_protocol = StreamProtocol::new("/agent/message/1.0.0");
            let rr_behavior = RequestResponseBehavior::<TransactionMessage, TransactionMessage>::new(
                [(rr_protocol, RequestResponseProtocolSupport::Full)],
                RequestResponseConfig::default()
            );

            // Create behavior directly
            Behavior::new(kad, identify, rr_behavior)
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(30)))
        .build();

    Ok(behavior)
}

// Fix the handle_event signature to use AgentEvent
async fn handle_event(
    _swarm: &mut Swarm<Behavior>,
    event: &SwarmEvent<AgentEvent>,
    peer_manager: &SharedPeerManager,
    _dag: Arc<TokioMutex<DAG>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match event {
        SwarmEvent::Behaviour(AgentEvent::Identify(identify_event)) => {
            if let IdentifyEvent::Received { peer_id, info: _ } = identify_event {
                let mut pm = peer_manager.lock().await;
                pm.peers.insert(*peer_id);
                info!("Peer identified and added to PeerManager: {:?}", peer_id);
                info!("Current peer list: {:?}", pm.peers.iter().collect::<Vec<_>>());
            }
        }
        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
            info!("Connection established with peer: {:?}", peer_id);
            let mut pm = peer_manager.lock().await;
            if let ConnectedPoint::Dialer { address: _, .. } = endpoint {
                pm.peers.insert(*peer_id);
            }
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            info!("Connection closed with peer: {:?}", peer_id);
            let mut pm = peer_manager.lock().await;
            pm.peers.remove(peer_id);
            info!("Current peer list: {:?}", pm.peers.iter().collect::<Vec<_>>());
        }
        _ => {}
    }
    Ok(())
}

// Fix the receive_transaction handler to use .await for TokioMutex
#[debug_handler]
async fn receive_transaction(
    State(state): State<SharedState>,
    Json(payload): Json<TransactionRequestData>,
) -> axum::response::Response {
    let (peer_manager, dag, swarm) = state;
    info!("Node received new transaction: {:?}", payload);

    // Get current peer list from PeerManager
    let peers_list = {
        let mut swarm_lock = swarm.lock().await;
        let pm = peer_manager.lock().await;
        swarm_lock.behaviour_mut().get_peers(&*pm)
    };
    
    info!("Peers available for transaction: {:?}", peers_list);

    // Create the transaction
    let transaction = Transaction::new(payload.data, payload.parents);

    // Add the transaction to the DAG
    let mut dag_lock = dag.lock().await;
    dag_lock.add_transaction(transaction.clone());

    if peers_list.is_empty() {
        return Json(json!({
            "status": "warning",
            "message": "Transaction received but no peers available for propagation"
        })).into_response();
    }

    // Prepare the transaction message
    let message = TransactionMessage {
        transaction_id: transaction.id.clone(),
        transaction_data: transaction.data.clone(),
        parents: transaction.parents.clone(),
    };

    // Lock swarm to propagate the transaction
    let mut swarm_lock = swarm.lock().await;
    let peer_count = peers_list.len();

    // Send to each peer
    for peer_id in &peers_list {
        info!("Sending transaction to peer: {:?}", peer_id);
        let outbound_id = swarm_lock.behaviour_mut().send_message(peer_id, message.clone());
        info!("Transaction {} propagated to peer {} with request ID {:?}", 
            transaction.id, peer_id, outbound_id);
    }

    // Return a successful HTTP response
    Json(json!({
        "status": "success",
        "message": "Transaction received and propagated",
        "peer_count": peer_count
    })).into_response()
}

impl PeerManagement for PeerManager {
    fn get_peers(&self) -> Vec<PeerId> {
        self.peers.iter().cloned().collect()
    }

    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        // Implement this method based on your PeerManager's functionality
        self.peers.insert(peer_id);
        // Add address handling if needed
    }
}




