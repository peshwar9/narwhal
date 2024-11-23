use std::collections::HashSet;
use libp2p::PeerId;
use log::info;

use serde::{Serialize, Deserialize};
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TransactionRequest {
    transaction_id: String,
    transaction_data: String,
    parents: Vec<String>,
}

pub struct PeerManager {
    pub peers: HashSet<PeerId>
}

impl PeerManager {
    pub fn new() -> Self {
        PeerManager {
            peers: HashSet::new(),
        }
    }

    pub fn has_peer(&self, peer_id: &PeerId) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn add_peer(&mut self, peer_id: PeerId) {
        if !self.has_peer(&peer_id) {
            self.peers.insert(peer_id);
            info!("PeerManager: Added peer {:?}, total peers: {}", peer_id, self.peers.len());
        }
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        if self.has_peer(peer_id) {
            self.peers.remove(peer_id);
            info!("PeerManager: Removed peer {:?}, total peers: {}", peer_id, self.peers.len());
        }
    }

    pub fn get_peers(&self) -> Vec<PeerId> {
        let peers = self.peers.iter().cloned().collect::<Vec<_>>();
        info!("PeerManager::get_peers returning {} peers: {:?}", peers.len(), peers);
        peers
    }



}