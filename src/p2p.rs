use crate::behavior::PeerManagement;
use std::collections::HashSet;
use libp2p::{PeerId, Multiaddr};
use std::fs;
use serde::{Serialize, Deserialize};
use std::path::Path;
use log::{info, error};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

// Custom serialization wrapper for PeerId
#[derive(Debug, Serialize, Deserialize)]
struct SerializablePeerId(String);

impl From<PeerId> for SerializablePeerId {
    fn from(peer_id: PeerId) -> Self {
        SerializablePeerId(peer_id.to_string())
    }
}

impl TryFrom<SerializablePeerId> for PeerId {
    type Error = String;
    fn try_from(spid: SerializablePeerId) -> Result<Self, Self::Error> {
        spid.0.parse().map_err(|e| format!("Failed to parse PeerId: {}", e))
    }
}

// Custom serialization wrapper for Multiaddr
#[derive(Debug, Serialize, Deserialize)]
struct SerializableMultiaddr(String);

impl From<Multiaddr> for SerializableMultiaddr {
    fn from(addr: Multiaddr) -> Self {
        SerializableMultiaddr(addr.to_string())
    }
}

impl TryFrom<SerializableMultiaddr> for Multiaddr {
    type Error = String;
    fn try_from(sma: SerializableMultiaddr) -> Result<Self, Self::Error> {
        sma.0.parse().map_err(|e| format!("Failed to parse Multiaddr: {}", e))
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PeerStorage {
    peers: HashSet<String>,  // Store peer IDs as strings
    addresses: HashMap<String, String>,  // Store addresses as strings
}

impl PeerStorage {
    pub fn new(peer_id: &PeerId) -> Self {
        let filename = format!("peers_{}.json", peer_id.to_base58());
        info!("Creating/loading peer storage from {}", filename);
        
        match fs::read_to_string(&filename) {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(storage) => {
                        info!("Loaded existing peer storage from {}", filename);
                        storage
                    }
                    Err(e) => {
                        error!("Failed to parse {}: {}. Creating new storage.", filename, e);
                        PeerStorage::default()
                    }
                }
            }
            Err(e) => {
                info!("No existing storage found ({}), creating new one", e);
                PeerStorage::default()
            }
        }
    }

    pub fn save_to_disk(&self, peer_id: &PeerId) -> Result<(), std::io::Error> {
        let filename = format!("peers_{}.json", peer_id.to_base58());
        info!("Saving peer storage to {}", filename);
        
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&filename, content)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct PeerManager {
    pub peers: HashSet<PeerId>,
    pub peer_addresses: HashMap<PeerId, Multiaddr>,
    local_peer_id: PeerId,
}

impl PeerManager {
    pub fn new(local_peer_id: PeerId) -> Self {
        info!("Creating new PeerManager for {:?}", local_peer_id);
        let storage = PeerStorage::new(&local_peer_id);
        
        let peers = storage.peers.iter()
            .filter_map(|p| p.parse::<PeerId>().ok())
            .collect();

        let peer_addresses = storage.addresses.iter()
            .filter_map(|(p, a)| {
                Some((
                    p.parse::<PeerId>().ok()?,
                    a.parse::<Multiaddr>().ok()?
                ))
            })
            .collect();

        PeerManager {
            peers,
            peer_addresses,
            local_peer_id,
        }
    }

    pub fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        info!("Adding peer {:?} with address {:?}", peer_id, addr);
        self.peers.insert(peer_id.clone());
        self.peer_addresses.insert(peer_id, addr);
        self.save_to_disk();
    }

    fn save_to_disk(&self) {
        let storage = PeerStorage {
            peers: self.peers.iter()
                .map(|p| p.to_base58().to_string())
                .collect(),
            addresses: self.peer_addresses.iter()
                .map(|(p, a)| (p.to_base58().to_string(), a.to_string()))
                .collect(),
        };
        
        if let Err(e) = storage.save_to_disk(&self.local_peer_id) {
            error!("Failed to save peer storage: {}", e);
        } else {
            info!("Successfully saved peers to disk for {:?}", self.local_peer_id);
        }
    }

    pub fn get_peers(&self) -> HashSet<PeerId> {
        let peers = self.peers.clone();
        info!("Retrieved {} peers from storage: {:?}", peers.len(), peers);
        peers
    }

    pub fn get_peer_addr(&self, peer_id: &PeerId) -> Option<&Multiaddr> {
        self.peer_addresses.get(peer_id)
    }
}

impl PeerManagement for PeerManager {
    fn get_peers(&self) -> Vec<PeerId> {
        self.get_peers().into_iter().collect()
    }

    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.add_peer_with_addr(peer_id, addr);
    }
}