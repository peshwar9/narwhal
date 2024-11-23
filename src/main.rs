use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use std::time::Duration;

use tokio;
use log::{info, error, warn};
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
    Event as IdentifyEvent
};

use libp2p::kad::{
    RoutingUpdate,
    Config as KadConfig, 
    Behaviour as KadBehavior, 
    Event as KadEvent,
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
    Behavior as AgentBehavior, 
    Event as AgentEvent
};

mod message;
use message::TransactionMessage;
use narhwal::transaction::Transaction;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    Builder::from_env(Env::default().default_filter_or("debug")).init();

    // Generate a local keypair
    let local_key = identity::Keypair::generate_ed25519();

    // Build the Swarm using the SwarmBuilder
    let mut swarm = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            TcpConfig::default(),
            NoiceConfig::new,
            YamuxConfig::default,
        )?
        .with_behaviour(|key| {
            let local_peer_id = PeerId::from(key.clone().public());
            info!("LocalPeerID: {local_peer_id}");

            // Configure Kademlia DHT
            let mut kad_config = KadConfig::default();
            kad_config.set_protocol_names(vec![StreamProtocol::new("/agent/connection/1.0.0")]);
            let kad_memory = KadInMemory::new(local_peer_id);
            let kad = KadBehavior::with_config(local_peer_id, kad_memory, kad_config);

            // Configure Identity Behavior
            let identity_config = IdentifyConfig::new(
                "/agent/connection/1.0.0".to_string(),
                key.clone().public(),
            )
            .with_push_listen_addr_updates(true)
            .with_interval(Duration::from_secs(30));

            // Configure Request/Response behavior
            let rr_config = RequestResponseConfig::default();
            let rr_protocol = StreamProtocol::new("/agent/message/1.0.0");
            let rr_behavior = RequestResponseBehavior::<TransactionMessage, TransactionMessage>::new(
                [(rr_protocol, RequestResponseProtocolSupport::Full)],
                rr_config,
            );
            let identify = IdentifyBehavior::new(identity_config);

            // Combine behaviors into a single AgentBehavior
            AgentBehavior::new(kad, identify, rr_behavior)
        })?
        .with_swarm_config(|cfg| {
            cfg.with_idle_connection_timeout(Duration::from_secs(30))
        })
        .build();

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

    let mut peers: HashMap<PeerId, Vec<Multiaddr>> = HashMap::new();
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => info!("NewListenAddr: {listener_id:?} | {address:?}"),
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                num_established,
                concurrent_dial_errors,
                established_in,
            } => info!(
                "ConnectionEstablished: {peer_id} | {connection_id} | {endpoint:?} | {num_established} | {concurrent_dial_errors:?} | {established_in:?}"
            ),
            SwarmEvent::Dialing {
                peer_id,
                connection_id,
            } => info!("Dialing: {peer_id:?} | {connection_id}"),
            SwarmEvent::Behaviour(AgentEvent::Identify(event)) => match event {
                IdentifyEvent::Sent { peer_id } => info!("IdentifyEvent:Sent: {peer_id}"),
                IdentifyEvent::Pushed { peer_id, info } => {
                    info!("IdentifyEvent:Pushed: {peer_id} | {info:?}")
                }
                IdentifyEvent::Received { peer_id, info } => {
                    info!("IdentifyEvent:Received: {peer_id} | {info:?}");
                    peers.insert(peer_id, info.clone().listen_addrs);

                    // Register addresses with Kademlia and Request/Response
                    for addr in info.clone().listen_addrs {
                        let agent_routing = swarm.behaviour_mut().register_addr_kad(&peer_id, addr.clone());
                        match agent_routing {
                            RoutingUpdate::Failed => error!("Failed to register address to Kademlia"),
                            RoutingUpdate::Pending => warn!("Register address pending"),
                            RoutingUpdate::Success => {
                                info!("Successfully registered address: {addr}")
                            }
                        }
                        _ = swarm.behaviour_mut().register_addr_rr(&peer_id, addr.clone());

                        let local_peer_id = local_key.public().to_peer_id();
// Create a sample Transaction with a sample transaction data
let transaction_data = format!("Send transaction from: {local_peer_id}: Some transaction data");

// Create the first transaction (without any parents for this example)
let transaction = Transaction::new(transaction_data.clone(), vec![]);  // vec![] means no parents for the first transaction

// Create the TransactionMessage using the generated transaction
let message = TransactionMessage {
    transaction_id: transaction.id,            // Use the ID generated by Transaction::new()
    transaction_data: transaction.data.clone(), // Use the data from the transaction
    parents: transaction.parents.clone(),      // Use the parents from the transaction
};
                        let request_id = swarm.behaviour_mut().send_message(&peer_id, message);
                        info!("RequestID: {request_id}");
                    }

                    info!("Available peers: {peers:?}");
                }
                _ => {}
            },
            SwarmEvent::Behaviour(AgentEvent::RequestResponse(event)) => match event {
                RequestResponseEvent::Message { peer, message } => {
                    match message {
                        RequestResponseMessage::Request {
                            request_id,
                            request,
                            channel,
                        } => {
                            info!(
                                "RequestResponseEvent::Message::Request -> PeerID: {peer} | RequestID: {request_id} | RequestMessage: {request:?}"
                            );
                            let local_peer_id = local_key.public().to_peer_id();
// Assuming you want to create a response with some transaction data
let data = format!("Response from: {local_peer_id}: transaction response");
// Construct the transaction message
let transaction = Transaction::new(data, vec![]);  // Empty parents vector for now, or you can add parent IDs

// Create the TransactionMessage response
let response = TransactionMessage {
    transaction_id: transaction.id,  // Use the ID generated by the Transaction struct
    transaction_data: transaction.data,  // The transaction data
    parents: transaction.parents,  // The parent transactions
};
                            let result = swarm.behaviour_mut().send_response(channel, response);
                            if result.is_err() {
                                let err = result.unwrap_err();
                                error!("Error sending response: {err:?}");
                            } else {
                                info!("Response sent successfully");
                            }
                        }
                        RequestResponseMessage::Response {
                            request_id,
                            response,
                        } => {
                            info!(
                                "RequestResponseEvent::Message::Response -> PeerID: {peer} | RequestID: {request_id} | Response: {response:?}"
                            );
                        }
                    }
                }
                RequestResponseEvent::InboundFailure {
                    peer,
                    request_id,
                    error,
                } => {
                    warn!(
                        "RequestResponseEvent::InboundFailure -> PeerID: {peer} | RequestID: {request_id} | Error: {error}"
                    );
                }
                RequestResponseEvent::ResponseSent {
                    peer,
                    request_id,
                } => {
                    info!(
                        "RequestResponseEvent::ResponseSent -> PeerID: {peer} | RequestID: {request_id}"
                    );
                }
                RequestResponseEvent::OutboundFailure {
                    peer,
                    request_id,
                    error,
                } => {
                    warn!(
                        "RequestResponseEvent::OutboundFailure -> PeerID: {peer} | RequestID: {request_id} | Error: {error}"
                    );
                }
            },
            SwarmEvent::Behaviour(AgentEvent::Kad(event)) => match event {
                KadEvent::ModeChanged { new_mode } => {
                    info!("KadEvent:ModeChanged: {new_mode}");
                }
                KadEvent::RoutablePeer { peer, address } => {
                    info!("KadEvent:RoutablePeer: {peer} | {address}");
                }
                KadEvent::PendingRoutablePeer { peer, address } => {
                    info!("KadEvent:PendingRoutablePeer: {peer} | {address}");
                }
                KadEvent::InboundRequest { request } => {
                    info!("KadEvent:InboundRequest: {request:?}");
                }
                KadEvent::RoutingUpdated {
                    peer,
                    is_new_peer,
                    addresses,
                    bucket_range,
                    old_peer,
                } => {
                    info!(
                        "KadEvent:RoutingUpdated: {peer} | {addresses:?} | New Peer: {is_new_peer}"
                    );
                },
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}
