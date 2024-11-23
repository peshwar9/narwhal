use libp2p::kad::RoutingUpdate;
use libp2p::kad::{
    store::MemoryStore as KademliaInMemory, Behaviour as KademliaBehavior, Event as KademliaEvent,
};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{futures, Multiaddr, PeerId};

use libp2p::identify::{Behaviour as IdentifyBehavior, Event as IdentifyEvent};

use libp2p::request_response::cbor::Behaviour as RequestResponseBehavior;
use libp2p::request_response::{
    Event as RequestResponseEvent, OutboundRequestId, ResponseChannel as RequestResponseChannel,
};

use crate::message::TransactionMessage; // Using TransactionMessage instead of GreetRequest
use narhwal::p2p::PeerManager;  // Add this import at the top

use log::{info, debug, error};

//use libp2p::request_response::{RequestResponseConfig, ProtocolSupport};
use std::iter;

use async_trait::async_trait;
use libp2p::request_response::Codec as RequestResponseCodec;
use futures::prelude::*;
use std::io;

use libp2p::futures::io::{AsyncRead, AsyncWrite};
use libp2p::futures::AsyncReadExt;
use libp2p::futures::AsyncWriteExt;
use unsigned_varint;
use serde_json;

const PROTOCOL_NAME: &str = "/transaction/1.0.0";

#[derive(Debug, Clone)]
struct TransactionCodec();

#[async_trait]
impl RequestResponseCodec for TransactionCodec {
    type Protocol = &'static str;
    type Request = TransactionMessage;
    type Response = TransactionMessage;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: libp2p::futures::AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io).await?;
        serde_json::from_slice(&vec)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: libp2p::futures::AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io).await?;
        serde_json::from_slice(&vec)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    async fn write_request<T>(&mut self, _: &Self::Protocol, io: &mut T, req: Self::Request) -> io::Result<()>
    where
        T: libp2p::futures::AsyncWrite + Unpin + Send,
    {
        let vec = serde_json::to_vec(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        write_length_prefixed(io, vec).await
    }

    async fn write_response<T>(&mut self, _: &Self::Protocol, io: &mut T, res: Self::Response) -> io::Result<()>
    where
        T: libp2p::futures::AsyncWrite + Unpin + Send,
    {
        let vec = serde_json::to_vec(&res)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        write_length_prefixed(io, vec).await
    }
}

// Helper functions for length-prefixed encoding/decoding
async fn read_length_prefixed<T>(io: &mut T) -> io::Result<Vec<u8>>
where
    T: libp2p::futures::AsyncRead + Unpin + Send,
{
    let length = unsigned_varint::aio::read_usize(&mut *io).await.map_err(|e| {
        io::Error::new(io::ErrorKind::Other, e)
    })?;
    let mut vec = vec![0; length];
    io.read_exact(&mut vec).await?;
    Ok(vec)
}

async fn write_length_prefixed<T>(io: &mut T, vec: Vec<u8>) -> io::Result<()>
where
    T: libp2p::futures::AsyncWrite + Unpin + Send,
{
    // Create a buffer for the length encoding
    let mut length_buf = [0u8; 10];  // 10 bytes is enough for usize
    let encoded = unsigned_varint::encode::usize(vec.len(), &mut length_buf);
    
    // Write the length prefix
    io.write_all(encoded).await?;
    // Write the actual data
    io.write_all(&vec).await?;
    io.flush().await?;
    Ok(())
}

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
        Self { 
            kad, 
            identify, 
            rr 
        }
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
            RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                error!("Failed to send transaction to peer {}: {:?}", peer, error);
            },
            RequestResponseEvent::ResponseSent { peer, .. } => {
                info!("Transaction successfully sent to peer {}", peer);
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
