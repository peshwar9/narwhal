use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use axum::response::IntoResponse;
use libp2p::swarm::Swarm;
use std::time::Duration;
use axum::extract::State;
use axum::{
    routing::post,
    Json, Router,
};
use serde::{Serialize, Deserialize};
use std::sync::{Arc, Mutex};

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

type _PeerMap = Arc<Mutex<HashMap<PeerId, Vec<Multiaddr>>>>;
type SharedState = (
    Arc<Mutex<Swarm<Behavior>>>,
    Arc<Mutex<HashMap<PeerId, Vec<Multiaddr>>>>,
    Arc<Mutex<DAG>>
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
    let dag = Arc::new(Mutex::new(DAG::new()));

    // Build the Swarm
    let mut swarm = build_swarm(local_key.clone(), dag.clone()).await?;

    // Set the server mode
    swarm.behaviour_mut().set_server_mode();

    // Listen and dial based on the provided argument
    if let Some(addr) = args().nth(1) {
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        info!("Dialed to: {addr}");
    } else {
        info!("Acting as bootstrap node");
        swarm.listen_on("/ip4/0.0.0.0/tcp/8000".parse()?)?;
    }

    // Handle events in the swarm
    handle_swarm_events(&mut swarm, dag.clone()).await?;

    // Create shared state
    let swarm = Arc::new(Mutex::new(swarm));
    let peers = Arc::new(Mutex::new(HashMap::new()));
    let state = (swarm.clone(), peers, dag);

    // Initialize the Axum server with state
    let app = Router::new()
        .route("/transaction", post(receive_transaction))
        .with_state(state);

    // Run Axum server in a separate task
    tokio::spawn(async move {
        axum::serve(
            tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap(),
            app.into_make_service()
        )
        .await
        .unwrap();
    });

    Ok(())
}

// Function to build and configure the swarm
async fn build_swarm(local_key: identity::Keypair, _dag: Arc<Mutex<DAG>>) -> Result<Swarm<Behavior>, Box<dyn std::error::Error>> {
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
// Function to handle swarm events in a loop
async fn handle_swarm_events(swarm: &mut Swarm<Behavior>, dag: Arc<Mutex<DAG>>) -> Result<(), Box<dyn Error>> {
    let mut peers: HashMap<PeerId, Vec<Multiaddr>> = HashMap::new();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { listener_id, address } => info!("NewListenAddr: {listener_id:?} | {address:?}"),
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                num_established,
                concurrent_dial_errors,
                established_in,
            } => info!("ConnectionEstablished: {peer_id} | {connection_id} | {endpoint:?} | {num_established} | {concurrent_dial_errors:?} | {established_in:?}"),
            SwarmEvent::Dialing { peer_id, connection_id } => info!("Dialing: {peer_id:?} | {connection_id}"),
            SwarmEvent::Behaviour(AgentEvent::Identify(event)) => match event {
                IdentifyEvent::Sent { peer_id } => info!("IdentifyEvent:Sent: {peer_id}"),
                IdentifyEvent::Pushed { peer_id, info } => {
                    info!("IdentifyEvent:Pushed: {peer_id} | {info:?}")
                }
                IdentifyEvent::Received { peer_id, info } => {
                    info!("IdentifyEvent:Received: {peer_id} | {info:?}");
                    peers.insert(peer_id, info.clone().listen_addrs);
                }
                _ => {}
            },
            SwarmEvent::Behaviour(AgentEvent::RequestResponse(event)) => match event {
                RequestResponseEvent::Message { peer, message } => {
                    match message {
                        RequestResponseMessage::Request {
                            request_id,
                            request,
                            channel: _,
                        } => {
                            info!("RequestResponseEvent::Message::Request -> PeerID: {peer} | RequestID: {request_id} | RequestMessage: {request:?}");

                            // Handle received transaction
                            let transaction = Transaction::new(request.transaction_data.clone(), request.parents.clone());
                            let mut dag_lock = dag.lock().unwrap();
                            dag_lock.add_transaction(transaction.clone());

                            // Propagate transaction to other nodes
                            // let response = TransactionMessage {
                            //     transaction_id: transaction.id,
                            //     transaction_data: transaction.data,
                            //     parents: transaction.parents,
                            // };
                            // let result = swarm.behaviour_mut().send_response(channel, response);
                            // if result.is_err() {
                            //     let err = result.unwrap_err();
                            //     error!("Error sending response: {err:?}");
                            // } else {
                            //     info!("Response sent successfully");
                            // }
                            info!("Transaction added to DAG without response to peer");

                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

// Axum handler for receiving transactions and propagating them

#[axum::debug_handler]
async fn receive_transaction(
    State(state): State<SharedState>,
    Json(payload): Json<TransactionRequestData>,
) -> impl IntoResponse {
    let (swarm, peers, dag) = state;
    info!("Received new transaction: {:?}", payload);

    let transaction = Transaction::new(payload.data, payload.parents);
    let mut dag_lock = dag.lock().unwrap();
    dag_lock.add_transaction(transaction.clone());

    let message = TransactionMessage {
        transaction_id: transaction.id.clone(),
        transaction_data: transaction.data.clone(),
        parents: transaction.parents.clone(),
    };

    for (peer_id, _) in peers.lock().unwrap().iter() {
        let mut swarm_lock = swarm.lock().unwrap();
        swarm_lock.behaviour_mut().send_message(peer_id, message.clone());
        info!("Transaction sent to peer {peer_id}");
    }

    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::from("Transaction received and being propagated"))
        .unwrap()
}

