use std::collections::HashSet;
use libp2p::PeerId;


use serde::{Serialize, Deserialize};
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TransactionRequest {
    transaction_id: String,
    transaction_data: String,
    parents: Vec<String>,
}

pub struct PeerManager {
    peers: HashSet<PeerId>
}

impl PeerManager {
    pub fn new() -> Self {
        PeerManager {
            peers: HashSet::new(),
        }
    }

    pub fn add_peer(&mut self, peer_id: PeerId) {
        self.peers.insert(peer_id);
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    pub fn get_peers(&self) -> Vec<PeerId> {
        self.peers.iter().cloned().collect()
    }



}