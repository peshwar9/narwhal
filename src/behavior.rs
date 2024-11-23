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

use crate::message::TransactionMessage; // Using TransactionMessage instead of GreetRequest
use narhwal::p2p::PeerManager;  // Add this import at the top

use log::{info, debug, error};

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

    pub fn _register_addr_kad(&mut self, peer_id: &PeerId, addr: Multiaddr) -> RoutingUpdate {
        self.kad.add_address(peer_id, addr)
    }

    pub fn _send_transaction_to_peers(&mut self, transaction: TransactionMessage, peer_manager: &PeerManager) {
        for peer in peer_manager.get_peers() {
            self.rr.send_request(&peer, transaction.clone());
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
            Event::Kad(e) => {
                debug!("Kad event: {:?}", e);
            },
            Event::Identify(e) => {
                debug!("Identify event: {:?}", e);
            }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum Event {
    Identify(IdentifyEvent),
    Kad(KademliaEvent),
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
