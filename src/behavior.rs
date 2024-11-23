use libp2p::kad::RoutingUpdate;
use libp2p::kad::{
    store::MemoryStore as KademliaInMemory, Behaviour as KademliaBehavior, Event as KademliaEvent,
};
use narhwal::p2p::PeerManager;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{Multiaddr, PeerId};

use libp2p::identify::{Behaviour as IdentifyBehavior, Event as IdentifyEvent};

use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;
use libp2p::request_response::{
    Event as RequestResponseEvent, OutboundRequestId, ResponseChannel as RequestResponseChannel,
};
use libp2p::swarm::SwarmEvent;

use std::collections::HashSet;
use crate::message::TransactionMessage; // Using TransactionMessage instead of GreetRequest


use log::{info, debug, error};

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
type SharedPeerManager = Arc<TokioMutex<PeerManager>>;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "Event")]
pub(crate) struct Behavior {
    identify: IdentifyBehavior,
    kad: KademliaBehavior<KademliaInMemory>,
    rr: RequestResponseBehavior<TransactionMessage, TransactionMessage>, // Changed from GreetRequest/GreetResponse

}

impl Behavior {
    pub fn new(
        kad: KademliaBehavior<KademliaInMemory>,
        identify: IdentifyBehavior,
        rr: RequestResponseBehavior<TransactionMessage, TransactionMessage>,
    ) -> Self {
        Self { kad, identify, rr }
    }
}

impl Behavior {
    pub fn _register_addr_kad(&mut self, peer_id: &PeerId, addr: Multiaddr) -> RoutingUpdate {
        self.kad.add_address(peer_id, addr)
    }

    pub async fn send_transaction_to_peers(&mut self, transaction: TransactionMessage, peer_manager: &SharedPeerManager) {
        let peers = {
            let pm = peer_manager.lock().await;
            pm.get_peers()
        };
        
        if peers.is_empty() {
            info!("No peers available to send transaction to");
            return;
        }
        
        info!("Sending transaction to {} peers", peers.len());
        for peer in &peers {
            info!("Sending transaction to peer: {:?}", peer);
            self.rr.send_request(peer, transaction.clone());
        }
    }

    pub fn send_message(
        &mut self,
        peer_id: &PeerId,
        message: TransactionMessage,
    ) -> OutboundRequestId {
        // Direct call to the request-response behavior's send_request
        self.rr.send_request(peer_id, message)
    }

    // Send a response with a TransactionMessage
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

    // Add this method to handle incoming messages
    fn handle_request_response_event(&mut self, event: RequestResponseEvent<TransactionMessage, TransactionMessage>) {
        match event {
            RequestResponseEvent::Message { 
                peer, 
                message 
            } => {
                info!("Received transaction from peer {}: {:?}", peer, message);
                // 1. Add the transaction to your DAG
                // 2. Propagate to other peers if needed
            },
            RequestResponseEvent::InboundFailure { 
                peer,
                error,
                .. 
            } => {
                error!("Inbound request failed from peer {}: {:?}", peer, error);
            },
            _ => {} // Handle other events if needed
        }
    }

    fn on_swarm_event(&mut self, event: Event) {
        match event {
            Event::RequestResponse(e) => self.handle_request_response_event(e),
            Event::Kad(kad_event) => {
                debug!("Kad event: {:?}", kad_event);
                match kad_event {
                    KademliaEvent::RoutingUpdated { peer, addresses, .. } => {
                        for address in addresses.iter() {
                            self.kad.add_address(&peer, address.clone());
                            info!("Kad: Added address {:?} for discovered peer {:?}", address, peer);
                        }
                    },
                    KademliaEvent::OutboundQueryProgressed { result, .. } => {
                        debug!("Kademlia query result: {:?}", result);
                    },
                    _ => debug!("Other Kademlia event: {:?}", kad_event),
                }
            },
            Event::ConnectionEstablished { peer_id, endpoint } => {
                self._register_addr_kad(&peer_id, endpoint.clone());
                info!("ConnectionEstablished: Added address {:?} for peer {:?}", endpoint, peer_id);
            },
            Event::Identify(identify_event) => {
                debug!("Identify event: {:?}", identify_event);
            },

            
        }
    }
    
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum Event {
    Identify(IdentifyEvent),
    Kad(KademliaEvent),
    RequestResponse(RequestResponseEvent<TransactionMessage, TransactionMessage>),
    ConnectionEstablished {
        peer_id: PeerId,
        endpoint: Multiaddr, // The address field is within the endpoint
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
    pub fn get_peers(&mut self) -> HashSet<PeerId> {
        self.kad
            .kbuckets()
            .into_iter()
            .flat_map(|bucket| {
                bucket.iter()
                    .map(|entry| *entry.node.key.preimage())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}



