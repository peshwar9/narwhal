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
use std::sync::{Arc, Mutex as StdMutex};
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
    Event as RequestResponseEvent,
    Message as RequestResponseMessage
};

use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;

mod behavior;
use behavior::{
    Behavior,
    Event as AgentEvent
};

mod message;
use message::TransactionMessage;
use narhwal::transaction::Transaction;
use narhwal::dag::DAG;

type _PeerMap = Arc<TokioMutex<HashMap<PeerId, Vec<Multiaddr>>>>;
type SharedState = (
    Arc<TokioMutex<HashMap<PeerId, Vec<Multiaddr>>>>,
    Arc<TokioMutex<DAG>>,
    Arc<TokioMutex<Swarm<Behavior>>>
);

#[derive(Serialize, Deserialize, Debug)]
struct TransactionRequestData {
    data: String,
    parents: Vec<String>,
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
        tokio::spawn(async move {
            loop {
                let event = {
                    let mut swarm = swarm_clone.lock().await;
                    swarm.select_next_some().await
                };

                // Create a new block to handle events
                {
                    let mut swarm = swarm_clone.lock().await;
                    if let Err(e) = handle_event(&mut swarm, &event, dag.clone()).await {
                        error!("Error handling swarm event: {}", e);
                        break;
                    }
                }
            }
        })
    };

    // Create shared state
    let peers = Arc::new(TokioMutex::new(HashMap::new()));
    let state = (peers.clone(), dag.clone(), swarm.clone());

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
    let swarm = SwarmBuilder::with_existing_identity(local_key.clone())
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

            Behavior::new(kad, identify, rr_behavior)
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(30)))
        .build();

    Ok(swarm)
}

// Fix the handle_event signature to use AgentEvent
async fn handle_event(
    _swarm: &mut Swarm<Behavior>,
    _event: &SwarmEvent<AgentEvent>,
    _dag: Arc<TokioMutex<DAG>>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    Ok(())
}

// Fix the receive_transaction handler to use .await for TokioMutex
async fn receive_transaction(
    State(state): State<SharedState>,
    Json(payload): Json<TransactionRequestData>,
) -> impl IntoResponse {
    let (peers, dag, swarm) = state;
    info!("Node received new transaction: {:?}", payload);

    let transaction = Transaction::new(payload.data, payload.parents);
    let mut dag_lock = dag.lock().await;
    dag_lock.add_transaction(transaction.clone());

    let message = TransactionMessage {
        transaction_id: transaction.id.clone(),
        transaction_data: transaction.data.clone(),
        parents: transaction.parents.clone(),
    };

    let peers_lock = peers.lock().await;
    for (peer_id, _) in peers_lock.iter() {
        let mut swarm_lock = swarm.lock().await;
        swarm_lock.behaviour_mut().send_message(peer_id, message.clone());
        info!("Transaction {} propagated to peer {}", transaction.id, peer_id);
    }

    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::from("Transaction received and being propagated"))
        .unwrap()
}

