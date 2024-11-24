use libp2p::kad::RoutingUpdate;
use libp2p::kad::{
    store::MemoryStore as KademliaInMemory, Behaviour as KademliaBehavior, Event as KademliaEvent,
};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{Multiaddr, PeerId};

use libp2p::identify::{Behaviour as IdentifyBehavior, Event as IdentifyEvent};

use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;
use libp2p::request_response::{
    Event as RequestResponseEvent, OutboundRequestId, ResponseChannel as RequestResponseChannel,
};

use crate::message::TransactionMessage;
use std::collections::HashSet;

use log::{debug, error, info};

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

// Define the trait here in behavior.rs
pub trait PeerManagement: std::fmt::Debug {
    fn get_peers(&self) -> Vec<PeerId>;
    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr);
}

// Make type alias generic over PeerManagement
type SharedPeerManager<T: PeerManagement> = Arc<TokioMutex<T>>;

use tokio::task::block_in_place;

// Update context to be generic
pub(crate) struct BehaviorContext<T: PeerManagement> {
    pub peer_manager: SharedPeerManager<T>,
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
pub struct Behavior {
    identify: IdentifyBehavior,
    kad: KademliaBehavior<KademliaInMemory>,
    rr: RequestResponseBehavior<TransactionMessage, TransactionMessage>,
}

// Update BehaviorWithContext to be generic
pub(crate) struct BehaviorWithContext<T: PeerManagement> {
    behavior: Behavior,
    context: Arc<BehaviorContext<T>>,
}

impl<T: PeerManagement> BehaviorWithContext<T> {
    pub fn new(
        kad: KademliaBehavior<KademliaInMemory>,
        identify: IdentifyBehavior,
        rr: RequestResponseBehavior<TransactionMessage, TransactionMessage>,
        peer_manager: SharedPeerManager<T>,
    ) -> Self {
        let context = Arc::new(BehaviorContext { peer_manager });

        let behavior = Behavior::new(kad, identify, rr);

        Self { behavior, context }
    }

    // Delegate methods to access the inner behavior and context
    pub fn behavior_mut(&mut self) -> &mut Behavior {
        &mut self.behavior
    }

    pub fn context(&self) -> &Arc<BehaviorContext<T>> {
        &self.context
    }

    pub async fn send_transaction_to_peers(&mut self, transaction: TransactionMessage) {
        let peers = {
            let pm = self.context.peer_manager.lock().await;
            info!("PeerManager state before sending transaction: {:?}", pm);
            pm.get_peers()
        };

        info!("Found {} peers for transaction propagation", peers.len());

        if peers.is_empty() {
            info!("No peers available to send transaction to");
            return;
        }

        for peer in &peers {
            info!("Attempting to send transaction to peer: {:?}", peer);
            self.behavior.rr.send_request(peer, transaction.clone());
        }
    }

    fn on_swarm_event(&mut self, event: Event) {
        match event {
            Event::Kad(kad_event) => match kad_event {
                KademliaEvent::RoutingUpdated {
                    peer, addresses, ..
                } => {
                    for address in addresses.iter() {
                        self.behavior.kad.add_address(&peer, address.clone());
                        block_in_place(|| {
                            let mut pm = self.context.peer_manager.blocking_lock();
                            pm.add_peer_with_addr(peer, address.clone());
                        });
                        info!(
                            "Kad: Added address {:?} for discovered peer {:?}",
                            address, peer
                        );
                    }
                }
                KademliaEvent::OutboundQueryProgressed { result, .. } => {
                    debug!("Kademlia query result: {:?}", result);
                }
                _ => debug!("Other Kademlia event: {:?}", kad_event),
            },
            Event::ConnectionEstablished { peer_id, endpoint } => {
                self.behavior._register_addr_kad(&peer_id, endpoint.clone());
                block_in_place(|| {
                    let mut pm = self.context.peer_manager.blocking_lock();
                    pm.add_peer_with_addr(peer_id, endpoint.clone());
                    info!(
                        "ConnectionEstablished: Added peer {:?} with address {:?}",
                        peer_id, endpoint
                    );
                });
            }
            Event::Identify(identify_event) => {
                if let IdentifyEvent::Received { peer_id, info, .. } = identify_event {
                    block_in_place(|| {
                        let mut pm = self.context.peer_manager.blocking_lock();
                        for addr in info.listen_addrs {
                            pm.add_peer_with_addr(peer_id, addr.clone());
                        }
                    });
                }
            }
            Event::RequestResponse(e) => self.behavior.handle_request_response_event(e),
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Identify(IdentifyEvent),
    Kad(KademliaEvent),
    RequestResponse(RequestResponseEvent<TransactionMessage, TransactionMessage>),
    ConnectionEstablished {
        peer_id: PeerId,
        endpoint: Multiaddr,
    },
}

pub(crate) enum BehaviorEvent {
    Identify(IdentifyEvent),
    Kademlia(KademliaEvent),
    RequestResponse(RequestResponseEvent<TransactionMessage, TransactionMessage>),
}

impl From<IdentifyEvent> for Event {
    fn from(value: IdentifyEvent) -> Self {
        Self::Identify(value)
    }
}

impl From<KademliaEvent> for Event {
    fn from(value: KademliaEvent) -> Self {
        Self::Kad(value)
    }
}

impl From<RequestResponseEvent<TransactionMessage, TransactionMessage>> for Event {
    // Changed from GreetRequest/GreetResponse
    fn from(value: RequestResponseEvent<TransactionMessage, TransactionMessage>) -> Self {
        Self::RequestResponse(value)
    }
}

impl Behavior {
    pub fn _register_addr_kad(&mut self, peer_id: &PeerId, addr: Multiaddr) -> RoutingUpdate {
        self.kad.add_address(peer_id, addr)
    }

    pub fn send_message(
        &mut self,
        peer_id: &PeerId,
        message: TransactionMessage,
    ) -> OutboundRequestId {
        self.rr.send_request(peer_id, message)
    }

    pub fn _send_response(
        &mut self,
        ch: RequestResponseChannel<TransactionMessage>,
        rs: TransactionMessage,
    ) -> Result<(), TransactionMessage> {
        self.rr.send_response(ch, rs)
    }

    pub fn set_server_mode(&mut self) {
        self.kad.set_mode(Some(libp2p::kad::Mode::Server))
    }

    fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<TransactionMessage, TransactionMessage>,
    ) {
        match event {
            RequestResponseEvent::Message { peer, message } => {
                debug!(
                    "[handle_request_response_event] Received transaction message: {:?}",
                    message
                );
                info!("Received transaction from peer {}: {:?}", peer, message);
                // TODO: Add transaction to DAG
            }
            RequestResponseEvent::InboundFailure { peer, error, .. } => {
                error!("Inbound request failed from peer {}: {:?}", peer, error);
            }
            RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                error!("Outbound request to peer {}: {:?}", peer, error);
            }
            _ => {}
        }
    }

    pub fn get_peers<T: PeerManagement>(&mut self, peer_manager: &T) -> HashSet<PeerId> {
        peer_manager.get_peers().into_iter().collect()
    }

    pub fn new(
        kad: KademliaBehavior<KademliaInMemory>,
        identify: IdentifyBehavior,
        rr: RequestResponseBehavior<TransactionMessage, TransactionMessage>,
    ) -> Self {
        Self { kad, identify, rr }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Identify(identify_event) => {
                if let IdentifyEvent::Received { peer_id, info, .. } = identify_event {
                    info!("Identified peer: {:?}", peer_id);
                    for addr in info.listen_addrs {
                        self.kad.add_address(&peer_id, addr);
                    }
                }
            }
            Event::Kad(kad_event) => {
                if let KademliaEvent::RoutingUpdated {
                    peer, addresses, ..
                } = kad_event
                {
                    for address in addresses.iter() {
                        info!("Kad: Added address {:?} for peer {:?}", address, peer);
                        self.kad.add_address(&peer, address.clone());
                    }
                }
            }
            Event::RequestResponse(event) => match event {
                RequestResponseEvent::Message { peer, message } => {
                    info!("Received message from peer {:?}: {:?}", peer, message);
                }
                RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                    error!("Outbound request to peer {:?} failed: {:?}", peer, error);
                }
                RequestResponseEvent::InboundFailure { peer, error, .. } => {
                    error!("Inbound request from peer {:?} failed: {:?}", peer, error);
                }
                _ => {}
            },
            Event::ConnectionEstablished { peer_id, endpoint } => {
                info!(
                    "Connection established with peer: {:?} at {:?}",
                    peer_id, endpoint
                );
                // Keep track of the peer but don't remove them on disconnect
            }
        }
    }
}
