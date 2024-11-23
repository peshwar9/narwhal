use std::collections::HashSet;
use libp2p::{PeerId, Multiaddr};
use log::info;
use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use std::str::FromStr;
use crate::behavior::PeerManagement;  // Import the trait from behavior

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
pub struct PeerData {
    pub addresses: Vec<Multiaddr>,
    pub last_seen: u64,
}

#[derive(Debug)]
pub struct PeerManager {
    pub peers: HashSet<PeerId>,
    pub peer_data: std::collections::HashMap<PeerId, PeerData>,
    storage_path: PathBuf,
}

impl PeerManager {
    pub fn new() -> Self {
        let storage_path = PathBuf::from("peers.json");
        info!("Loading peers from: {:?}", storage_path);
        let peer_data = Self::load_peers(&storage_path).unwrap_or_default();
        let peers = peer_data.keys().cloned().collect();
        
        let manager = PeerManager {
            peers,
            peer_data,
            storage_path,
        };
        
        // Log the loaded peers
        info!("Loaded {} peers from storage", manager.peers.len());
        for (peer_id, data) in &manager.peer_data {
            info!("Loaded peer: {} with {} addresses", peer_id, data.addresses.len());
        }
        
        manager
    }

    pub fn load_peers(path: &PathBuf) -> Result<std::collections::HashMap<PeerId, PeerData>, Box<dyn std::error::Error>> {
        if path.exists() {
            let data = fs::read_to_string(path)?;
            info!("Read peer data from disk: {}", data);
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

    pub fn save_peers(&self) -> Result<(), Box<dyn std::error::Error>> {
        let serializable: Vec<SerializablePeer> = self.peer_data
            .iter()
            .map(|(peer_id, data)| SerializablePeer {
                peer_id: peer_id.to_string(),
                addresses: data.addresses.iter().map(|a| a.to_string()).collect(),
                last_seen: data.last_seen,
            })
            .collect();
            
        let data = serde_json::to_string_pretty(&serializable)?;
        info!("Writing peer data to disk: {}", data);
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
                peer_data.addresses.push(addr.clone());
                info!("Added new address {} for peer {}", addr, peer_id);
            }
            peer_data.last_seen = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
        } else {
            self.add_peer(peer_id);
            if let Some(peer_data) = self.peer_data.get_mut(&peer_id) {
                peer_data.addresses.push(addr.clone());
                info!("Added first address {} for new peer {}", addr, peer_id);
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

// Then implement it for PeerManager
impl PeerManagement for PeerManager {
    fn get_peers(&self) -> Vec<PeerId> {
        self.peers.iter().cloned().collect()
    }

    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.add_peer_with_addr(peer_id, addr);
    }
}