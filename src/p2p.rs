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

impl PeerStorage {
    pub fn new(peer_id: &PeerId) -> Self {
        // Create a unique filename for each peer
        let filename = format!("peers_{}.json", peer_id.to_base58());
        
        // Try to load existing peers from the unique file
        if let Ok(content) = fs::read_to_string(&filename) {
            if let Ok(storage) = serde_json::from_str::<PeerStorage>(&content) {
                return storage;
            }
        }
        
        // Return empty storage if file doesn't exist or can't be parsed
        PeerStorage::default()
    }

    pub fn save_to_disk(&self, peer_id: &PeerId) {
        let filename = format!("peers_{}.json", peer_id.to_base58());
        if let Ok(content) = serde_json::to_string_pretty(self) {
            if let Ok(mut file) = File::create(&filename) {
                if let Err(e) = file.write_all(content.as_bytes()) {
                    error!("Failed to write peer storage to {}: {}", filename, e);
                }
            }
        }
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
        let storage = PeerStorage::new(&local_peer_id);
        
        // Convert SerializablePeerId to PeerId
        let peers = storage.peers.into_iter()
            .filter_map(|spid| {
                PeerId::try_from(spid).ok()
            })
            .collect::<HashSet<_>>();

        // Convert SerializableMultiaddr to Multiaddr
        let peer_addresses = storage.addresses.into_iter()
            .filter_map(|(peer_str, sma)| {
                let peer_id = peer_str.parse::<PeerId>().ok()?;
                let addr = Multiaddr::try_from(sma).ok()?;
                Some((peer_id, addr))
            })
            .collect::<HashMap<_, _>>();

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
                .map(|p| SerializablePeerId::from(p.clone()))
                .collect(),
            addresses: self.peer_addresses.iter()
                .map(|(p, a)| (p.to_string(), SerializableMultiaddr::from(a.clone())))
                .collect(),
        };
        storage.save_to_disk(&self.local_peer_id);
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