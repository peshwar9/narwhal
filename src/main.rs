use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use axum::response::IntoResponse;
use libp2p::swarm::Swarm;
use std::time::Duration;
use axum::extract::State;
use log::error;
use axum::{
    routing::{post, get},
    Json, Router,
};
use log::debug;
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
type SharedDAG = Arc<TokioMutex<DAG>>;
type SharedSwarm = Arc<TokioMutex<Swarm<Behavior>>>;
type SharedPeerManager = Arc<TokioMutex<PeerManager>>;

type SharedState = (SharedPeerManager, SharedDAG, SharedSwarm);

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

#[derive(Clone)]
pub struct AppState {
    peer_manager: Arc<TokioMutex<PeerManager>>,
    dag: Arc<TokioMutex<DAG>>,
    swarm: Arc<TokioMutex<Swarm<Behavior>>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    Builder::from_env(Env::default().default_filter_or("debug")).init();

    // Generate a local keypair
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    
    // Create initial state components
    let peer_manager = Arc::new(TokioMutex::new(PeerManager::new(local_peer_id)));
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

    // Wrap swarm in Arc<Mutex>
    let swarm = Arc::new(TokioMutex::new(swarm));

    // Create AppState for both tasks
    let state = AppState {
        peer_manager: peer_manager.clone(),
        dag: dag.clone(),
        swarm: swarm.clone(),
    };

    // Spawn the swarm task
    let swarm_task = {
        let swarm_clone = swarm.clone();
        let state = state.clone();

        tokio::spawn(async move {
            loop {
                let event = {
                    let mut swarm = swarm_clone.lock().await;
                    swarm.select_next_some().await
                };

                // Create a new block to handle events
                {
                    let mut swarm = swarm_clone.lock().await;
                    if let Err(e) = handle_event(&mut swarm, &event, &state).await {
                        error!("Error handling swarm event: {}", e);
                        break;
                    }
                }
            }
        })
    };

    // Initialize the Axum server with state
    let app = Router::new()
        .route("/transaction", post(receive_transaction))
        .route("/dag/state", get(get_dag_state))
        .with_state(state);

    // Get HTTP port from args or use default
    let http_port = match args().nth(2) {
        Some(port) => port,
        None => "3000".to_string()
    };

    // Spawn the HTTP server
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

// Update the handle_event signature
async fn handle_event(
    swarm: &mut Swarm<Behavior>,
    event: &SwarmEvent<AgentEvent>,
    state: &AppState,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("[handle_event] Processing event: {:?}", event);
    
    match event {
        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
            debug!("[handle_event] Connection established with {:?}", peer_id);
            let mut pm = state.peer_manager.lock().await;
            debug!("[handle_event] Current peers before add: {:?}", pm.get_peers());
            
            if let ConnectedPoint::Dialer { address, .. } = endpoint {
                pm.add_peer_with_addr(*peer_id, address.clone());
                debug!("[handle_event] Added peer with address");
            }
            
            debug!("[handle_event] Current peers after add: {:?}", pm.get_peers());
        }
        SwarmEvent::Behaviour(AgentEvent::Identify(identify_event)) => {
            if let IdentifyEvent::Received { peer_id, info } = identify_event {
                let mut pm = state.peer_manager.lock().await;
                // Add peer with their address
                if let Some(addr) = info.listen_addrs.first() {
                    pm.add_peer_with_addr(*peer_id, addr.clone());
                    info!("Peer identified and added to PeerManager with address: {:?} - {:?}", peer_id, addr);
                }
                info!("Current peer list: {:?}", pm.peers.iter().collect::<Vec<_>>());
            }
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            info!("Connection closed with peer: {:?}", peer_id);
            // Don't remove the peer from storage when connection closes
            let pm = state.peer_manager.lock().await;
            info!("Current peer list: {:?}", pm.peers.iter().collect::<Vec<_>>());
        }
        _ => {}
    }
    Ok(())
}

// Fix the receive_transaction handler to use .await for TokioMutex
#[debug_handler]
async fn receive_transaction(
    State(state): State<AppState>,
    Json(request): Json<TransactionRequestData>,
) -> impl IntoResponse {
    debug!("[receive_transaction] Received request: {:?}", request);
    
    let peers = {
        let pm = state.peer_manager.lock().await;
        debug!("[receive_transaction] PeerManager state before get_peers: {:?}", &*pm);
        pm.get_peers()
    };

    debug!("[receive_transaction] Available peers: {:?}", peers);

    if peers.is_empty() {
        return Json(json!({
            "status": "warning",
            "message": "Transaction received but no peers available for propagation"
        }));
    }

    // Add transaction to DAG
    let transaction = Transaction::new(request.data.clone(), request.parents.clone());
    let mut dag = state.dag.lock().await;
    dag.add_transaction(transaction);

    // Log success with peer count
    info!("Transaction added to DAG and will be propagated to {} peers", peers.len());

    Json(json!({
        "status": "success",
        "message": "Transaction received and propagated",
        "peers": peers.len()
    }))
}

// Update the handler signature to use AppState
async fn get_dag_state(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let dag = state.dag.lock().await;
    let transactions = dag.get_all_transactions();
    
    Json(json!({
        "transaction_count": transactions.len(),
        "transactions": transactions.iter().map(|tx| {
            json!({
                "data": tx.data(),
                "parents": tx.parents(),
                "hash": tx.hash()
            })
        }).collect::<Vec<_>>()
    }))
}

impl PeerManagement for PeerManager {
    fn get_peers(&self) -> Vec<PeerId> {
        self.peers.iter().cloned().collect()
    }

    fn add_peer_with_addr(&mut self, peer_id: PeerId, _addr: Multiaddr) {
        // Implement this method based on your PeerManager's functionality
        self.peers.insert(peer_id);
        // Add address handling if needed
    }
}




