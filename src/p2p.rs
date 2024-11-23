use std::collections::HashSet;
use libp2p::{PeerId, Multiaddr};
use log::info;
use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use std::str::FromStr;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TransactionRequest {
    transaction_id: String,
    transaction_data: String,
    parents: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SerializablePeer {
    peer_id: String,
    addresses: Vec<String>,
    last_seen: u64,
}

#[derive(Debug)]
struct PeerData {
    addresses: Vec<Multiaddr>,
    last_seen: u64,
}

#[derive(Debug)]
pub struct PeerManager {
    pub peers: HashSet<PeerId>,
    peer_data: std::collections::HashMap<PeerId, PeerData>,
    storage_path: PathBuf,
}

impl PeerManager {
    pub fn new() -> Self {
        let storage_path = PathBuf::from("peers.json");
        let peer_data = Self::load_peers(&storage_path).unwrap_or_default();
        let peers = peer_data.keys().cloned().collect();
        
        PeerManager {
            peers,
            peer_data,
            storage_path,
        }
    }

    fn load_peers(path: &PathBuf) -> Result<std::collections::HashMap<PeerId, PeerData>, Box<dyn std::error::Error>> {
        if path.exists() {
            let data = fs::read_to_string(path)?;
            let serialized_peers: Vec<SerializablePeer> = serde_json::from_str(&data)?;
            
            let mut peer_data = std::collections::HashMap::new();
            for sp in serialized_peers {
                if let (Ok(peer_id), Ok(addresses)) = (
                    PeerId::from_str(&sp.peer_id),
                    sp.addresses.into_iter().map(|a| Multiaddr::from_str(&a)).collect::<Result<Vec<_>, _>>()
                ) {
                    peer_data.insert(peer_id, PeerData {
                        addresses,
                        last_seen: sp.last_seen,
                    });
                }
            }
            Ok(peer_data)
        } else {
            Ok(std::collections::HashMap::new())
        }
    }

    fn save_peers(&self) -> Result<(), Box<dyn std::error::Error>> {
        let serializable: Vec<SerializablePeer> = self.peer_data
            .iter()
            .map(|(peer_id, data)| SerializablePeer {
                peer_id: peer_id.to_string(),
                addresses: data.addresses.iter().map(|a| a.to_string()).collect(),
                last_seen: data.last_seen,
            })
            .collect();
            
        let data = serde_json::to_string_pretty(&serializable)?;
        fs::write(&self.storage_path, data)?;
        Ok(())
    }

    pub fn has_peer(&self, peer_id: &PeerId) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn add_peer(&mut self, peer_id: PeerId) {
        if !self.has_peer(&peer_id) {
            self.peers.insert(peer_id);
            
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            self.peer_data.insert(peer_id, PeerData {
                addresses: Vec::new(),
                last_seen: now,
            });

            if let Err(e) = self.save_peers() {
                info!("Failed to save peers to disk: {}", e);
            }

            info!("PeerManager: Added peer {:?}, total peers: {}", peer_id, self.peers.len());
        }
    }

    pub fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        if let Some(peer_data) = self.peer_data.get_mut(&peer_id) {
            if !peer_data.addresses.contains(&addr) {
                peer_data.addresses.push(addr);
            }
            peer_data.last_seen = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
        } else {
            self.add_peer(peer_id);
            if let Some(peer_data) = self.peer_data.get_mut(&peer_id) {
                peer_data.addresses.push(addr);
            }
        }

        if let Err(e) = self.save_peers() {
            info!("Failed to save peers to disk: {}", e);
        }
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        if self.has_peer(peer_id) {
            self.peers.remove(peer_id);
            self.peer_data.remove(peer_id);

            if let Err(e) = self.save_peers() {
                info!("Failed to save peers to disk: {}", e);
            }

            info!("PeerManager: Removed peer {:?}, total peers: {}", peer_id, self.peers.len());
        }
    }

    pub fn get_peers(&self) -> Vec<PeerId> {
        // Filter out peers that haven't been seen in the last 24 hours
        let day_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - 86400;

        let peers = self.peer_data
            .iter()
            .filter(|(_, data)| data.last_seen > day_ago)
            .map(|(peer_id, _)| *peer_id)
            .collect::<Vec<_>>();

        info!("PeerManager::get_peers returning {} peers: {:?}", peers.len(), peers);
        peers
    }

    pub fn get_peer_addresses(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        self.peer_data.get(peer_id).map(|data| data.addresses.clone())
    }
}