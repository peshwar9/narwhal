use crate::behavior::PeerManagement;
use std::collections::HashSet;
use libp2p::{PeerId, Multiaddr};
use std::fs;
use serde::{Serialize, Deserialize};
use std::path::Path;
use log::{info, error};
use std::collections::HashMap;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerStorage {
    peers: Vec<SerializablePeerId>,
    addresses: HashMap<String, SerializableMultiaddr>,
}

impl Default for PeerStorage {
    fn default() -> Self {
        Self {
            peers: Vec::new(),
            addresses: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct PeerManager {
    pub peers: HashSet<PeerId>,
    pub peer_addresses: HashMap<PeerId, Multiaddr>,
    storage_path: String,
}

impl PeerManager {
    pub fn new(storage_path: &str) -> Self {
        info!("Loading peers from: {:?}", storage_path);
        let storage = Self::load_from_disk(storage_path);
        
        let mut peers = HashSet::new();
        let mut peer_addresses = HashMap::new();
        
        // Convert from storage format to runtime format
        for spid in storage.peers {
            if let Ok(peer_id) = PeerId::try_from(spid) {
                peers.insert(peer_id);
            }
        }
        
        for (peer_str, sma) in storage.addresses {
            if let (Ok(peer_id), Ok(addr)) = (peer_str.parse(), Multiaddr::try_from(sma)) {
                peer_addresses.insert(peer_id, addr);
            }
        }
        
        Self {
            peers,
            peer_addresses,
            storage_path: storage_path.to_string(),
        }
    }

    fn load_from_disk(path: &str) -> PeerStorage {
        match fs::read_to_string(path) {
            Ok(contents) => {
                match serde_json::from_str(&contents) {
                    Ok(storage) => {
                        info!("Loaded peer storage successfully");
                        storage
                    }
                    Err(e) => {
                        error!("Error parsing peer storage: {}", e);
                        PeerStorage::default()
                    }
                }
            }
            Err(e) => {
                info!("No existing peer storage found: {}", e);
                PeerStorage::default()
            }
        }
    }

    fn save_to_disk(&self) {
        let storage = PeerStorage {
            peers: self.peers.iter().cloned().map(SerializablePeerId::from).collect(),
            addresses: self.peer_addresses.iter()
                .map(|(k, v)| (k.to_string(), SerializableMultiaddr::from(v.clone())))
                .collect(),
        };

        match serde_json::to_string_pretty(&storage) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.storage_path, json) {
                    error!("Failed to save peers to disk: {}", e);
                }
            }
            Err(e) => error!("Failed to serialize peer storage: {}", e),
        }
    }

    pub fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.peers.insert(peer_id);
        self.peer_addresses.insert(peer_id, addr.clone());
        self.save_to_disk();
        info!("Added and saved peer {:?} with address {:?}", peer_id, addr);
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.peer_addresses.remove(peer_id);
        self.save_to_disk();
        info!("Removed and saved peer {:?}", peer_id);
    }

    pub fn get_peers(&self) -> HashSet<PeerId> {
        info!("Retrieved {} peers", self.peers.len());
        self.peers.clone()
    }

    pub fn get_peer_addr(&self, peer_id: &PeerId) -> Option<&Multiaddr> {
        self.peer_addresses.get(peer_id)
    }
}

impl PeerManagement for PeerManager {
    fn get_peers(&self) -> Vec<PeerId> {
        self.peers.iter().cloned().collect()
    }

    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.add_peer_with_addr(peer_id, addr);
    }
}