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
        info!("Initializing PeerManager with storage at: {}", storage_path);
        
        // Ensure the path is valid and accessible
        if let Err(e) = File::create(storage_path) {
            error!("Failed to create initial peer storage file: {}", e);
        }
        
        let storage = Self::load_from_disk(storage_path);
        
        // Convert serializable types to runtime types
        let peers: HashSet<PeerId> = storage.peers.into_iter()
            .filter_map(|spid| {
                match PeerId::try_from(spid) {
                    Ok(peer_id) => Some(peer_id),
                    Err(e) => {
                        error!("Failed to parse stored PeerId: {}", e);
                        None
                    }
                }
            })
            .collect();

        let peer_addresses: HashMap<PeerId, Multiaddr> = storage.addresses.into_iter()
            .filter_map(|(peer_str, sma)| {
                match (peer_str.parse(), Multiaddr::try_from(sma)) {
                    (Ok(peer_id), Ok(addr)) => Some((peer_id, addr)),
                    (Err(e), _) => {
                        error!("Failed to parse stored PeerId string: {}", e);
                        None
                    },
                    (_, Err(e)) => {
                        error!("Failed to parse stored Multiaddr: {}", e);
                        None
                    }
                }
            })
            .collect();

        let manager = Self {
            peers,
            peer_addresses,
            storage_path: storage_path.to_string(),
        };
        
        // Try to save immediately to verify we can write
        manager.save_to_disk();
        
        manager
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
        // Create directory if it doesn't exist
        if let Some(parent) = Path::new(&self.storage_path).parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    error!("Failed to create directory for peer storage: {}", e);
                    return;
                }
            }
        }

        let storage = PeerStorage {
            peers: self.peers.iter().cloned().map(SerializablePeerId::from).collect(),
            addresses: self.peer_addresses.iter()
                .map(|(k, v)| (k.to_string(), SerializableMultiaddr::from(v.clone())))
                .collect(),
        };

        // Try to create/open the file first
        let file = match File::create(&self.storage_path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create/open peer storage file {}: {}", self.storage_path, e);
                return;
            }
        };

        // Then try to serialize and write
        match serde_json::to_writer_pretty(file, &storage) {
            Ok(_) => info!("Successfully saved peers to {}", self.storage_path),
            Err(e) => error!("Failed to write peer storage: {}", e),
        }
    }

    pub fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.peers.insert(peer_id);
        self.peer_addresses.insert(peer_id, addr.clone());
        self.save_to_disk();
        info!("Added and saved peer {:?} with address {:?}", peer_id, addr);
        info!("Current peers in storage: {:?}", self.peers);
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.peer_addresses.remove(peer_id);
        self.save_to_disk();
        info!("Removed and saved peer {:?}", peer_id);
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