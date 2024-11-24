use axum::extract::State;
use axum::response::IntoResponse;
use axum::{
    routing::{get, post},
    Json, Router,
};
use libp2p::swarm::Swarm;
use log::debug;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;

use env_logger::{Builder, Env};
use log::info;

use libp2p::{
    identity, tcp::Config as TcpConfig, yamux::Config as YamuxConfig, Multiaddr, PeerId,
    StreamProtocol, SwarmBuilder,
};

use libp2p::futures::StreamExt;
use libp2p::noise::Config as NoiceConfig;
use libp2p::swarm::SwarmEvent;

use libp2p::identify::{
    Behaviour as IdentifyBehavior, Config as IdentifyConfig, Event as IdentifyEvent,
};

use libp2p::kad::{
    store::MemoryStore as KadInMemory, Behaviour as KademliaBehavior, Config as KadConfig,
};

use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;
use libp2p::request_response::{
    Config as RequestResponseConfig, Event as RequestResponseEvent,
    Message as RequestResponseMessage, ProtocolSupport,
};
use narwhal::p2p::PeerManager;

mod behavior;
use behavior::{Behavior, BehaviorContext, Event as AgentEvent, PeerManagement};

mod message;
use message::TransactionMessage;
use narwhal::dag::DAG;
use narwhal::transaction::Transaction;

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
        None => "3000".to_string(),
    };

    // Spawn the HTTP server
    let server = tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", http_port);
        info!("Starting HTTP server on {}", addr);
        axum::serve(
            tokio::net::TcpListener::bind(&addr)
                .await
                .expect("Failed to bind"),
            app.into_make_service(),
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
async fn build_swarm(
    local_key: identity::Keypair,
    _dag: Arc<TokioMutex<DAG>>,
) -> Result<Swarm<Behavior>, Box<dyn std::error::Error>> {
    let behavior = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(TcpConfig::default(), NoiceConfig::new, YamuxConfig::default)?
        .with_behaviour(|key| {
            let local_peer_id = PeerId::from(key.public());
            info!("LocalPeerID: {local_peer_id}");

            let kad_memory = KadInMemory::new(local_peer_id);
            let kad =
                KademliaBehavior::with_config(local_peer_id, kad_memory, KadConfig::default());

            let identify = IdentifyBehavior::new(
                IdentifyConfig::new("/agent/connection/1.0.0".to_string(), key.public())
                    .with_push_listen_addr_updates(true)
                    .with_interval(Duration::from_secs(30)),
            );

            let rr_protocol = StreamProtocol::new("/agent/message/1.0.0");
            let rr_behavior =
                RequestResponseBehavior::<TransactionMessage, TransactionMessage>::new(
                    [(rr_protocol, ProtocolSupport::Full)],
                    RequestResponseConfig::default(),
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
        SwarmEvent::ConnectionEstablished {
            peer_id, endpoint, ..
        } => {
            debug!("[handle_event] Connection established with {:?}", peer_id);
            let mut pm = state.peer_manager.lock().await;
            debug!(
                "[handle_event] Current peers before add: {:?}",
                pm.get_peers()
            );

            if let ConnectedPoint::Dialer { address, .. } = endpoint {
                pm.add_peer_with_addr(*peer_id, address.clone());
                debug!("[handle_event] Added peer with address");
            }

            debug!(
                "[handle_event] Current peers after add: {:?}",
                pm.get_peers()
            );
        }
        SwarmEvent::Behaviour(AgentEvent::RequestResponse(event)) => {
            match event {
                RequestResponseEvent::Message { peer, message } => {
                    info!("Received transaction from peer {}: {:?}", peer, message);

                    let msg = match message {
                        RequestResponseMessage::Request {
                            request_id: _, // Ignore unused
                            request,
                            channel,
                        } => {
                            // Move the channel out of the reference
                            let ch = unsafe { std::ptr::read(channel) };
                            // Send response to acknowledge receipt
                            if let Err(e) =
                                swarm.behaviour_mut()._send_response(ch, request.clone())
                            {
                                error!("Failed to send response to peer {}: {:?}", peer, e);
                            }
                            request
                        }
                        RequestResponseMessage::Response {
                            request_id: _, // Ignore unused
                            response,
                        } => response,
                    };

                    // Create transaction from message
                    let transaction =
                        Transaction::new(msg.transaction_data.clone(), msg.parents.clone());

                    // Add to DAG
                    let mut dag = state.dag.lock().await;
                    dag.add_transaction(transaction);

                    debug!("Added transaction to DAG from peer {}", peer);
                }
                RequestResponseEvent::InboundFailure { peer, error, .. } => {
                    error!("Inbound request failed from peer {}: {:?}", peer, error);
                }
                RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                    error!("Outbound request failed to peer {}: {:?}", peer, error);
                }
                _ => {}
            }
        }
        SwarmEvent::Behaviour(AgentEvent::Identify(identify_event)) => {
            if let IdentifyEvent::Received { peer_id, info } = identify_event {
                let mut pm = state.peer_manager.lock().await;
                // Add peer with their address
                if let Some(addr) = info.listen_addrs.first() {
                    pm.add_peer_with_addr(*peer_id, addr.clone());
                    info!(
                        "Peer identified and added to PeerManager with address: {:?} - {:?}",
                        peer_id, addr
                    );
                }
                info!(
                    "Current peer list: {:?}",
                    pm.peers.iter().collect::<Vec<_>>()
                );
            }
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            info!("Connection closed with peer: {:?}", peer_id);
            // Don't remove the peer from storage when connection closes
            let pm = state.peer_manager.lock().await;
            info!(
                "Current peer list: {:?}",
                pm.peers.iter().collect::<Vec<_>>()
            );
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
        debug!(
            "[receive_transaction] PeerManager state before get_peers: {:?}",
            &*pm
        );
        pm.get_peers()
    };

    debug!("[receive_transaction] Available peers: {:?}", peers);

    // Add transaction to DAG
    let transaction = Transaction::new(request.data.clone(), request.parents.clone());
    let mut dag = state.dag.lock().await;
    dag.add_transaction(transaction.clone());

    // Create transaction message
    let message = TransactionMessage {
        transaction_id: transaction.id,
        transaction_data: transaction.data,
        parents: transaction.parents,
    };

    // Send to all peers using swarm
    let mut swarm = state.swarm.lock().await;
    let peer_count = peers.len();
    for peer in &peers {
        debug!(
            "[receive_transaction] Sending transaction to peer: {:?}",
            peer
        );
        let request_id = swarm.behaviour_mut().send_message(peer, message.clone());
        debug!(
            "[receive_transaction] Sent transaction to peer {} with request_id {:?}",
            peer, request_id
        );
    }

    info!(
        "Transaction added to DAG and propagated to {} peers",
        peer_count
    );

    Json(json!({
        "status": "success",
        "message": "Transaction received and propagated",
        "peers": peer_count
    }))
}

// Update the handler signature to use AppState
async fn get_dag_state(State(state): State<AppState>) -> impl IntoResponse {
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
