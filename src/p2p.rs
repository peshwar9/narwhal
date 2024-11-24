use crate::behavior::PeerManagement;
use libp2p::{Multiaddr, PeerId};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

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
        spid.0
            .parse()
            .map_err(|e| format!("Failed to parse PeerId: {}", e))
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
        sma.0
            .parse()
            .map_err(|e| format!("Failed to parse Multiaddr: {}", e))
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PeerStorage {
    peers: HashSet<String>,             // Store peer IDs as strings
    addresses: HashMap<String, String>, // Store addresses as strings
}

impl PeerStorage {
    pub fn new(peer_id: &PeerId) -> Self {
        let filename = format!("peers_{}.json", peer_id.to_base58());
        info!("Creating/loading peer storage from {}", filename);

        match fs::read_to_string(&filename) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(storage) => {
                    info!("Loaded existing peer storage from {}", filename);
                    storage
                }
                Err(e) => {
                    error!("Failed to parse {}: {}. Creating new storage.", filename, e);
                    PeerStorage::default()
                }
            },
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
        info!(
            "[PeerManager::new] Creating new instance for {:?}",
            local_peer_id
        );
        let storage = PeerStorage::new(&local_peer_id);

        let peers = storage
            .peers
            .iter()
            .filter_map(|p| match p.parse::<PeerId>() {
                Ok(peer_id) => {
                    info!("[PeerManager::new] Loaded peer: {:?}", peer_id);
                    Some(peer_id)
                }
                Err(e) => {
                    error!("[PeerManager::new] Failed to parse peer ID {}: {}", p, e);
                    None
                }
            })
            .collect();

        let manager = PeerManager {
            peers,
            peer_addresses: storage
                .addresses
                .iter()
                .filter_map(|(p, a)| {
                    let peer_id = p.parse::<PeerId>().ok()?;
                    let addr = a.parse::<Multiaddr>().ok()?;
                    info!(
                        "[PeerManager::new] Loaded peer address: {:?} -> {:?}",
                        peer_id, addr
                    );
                    Some((peer_id, addr))
                })
                .collect(),
            local_peer_id,
        };

        info!(
            "[PeerManager::new] Created with {} peers",
            manager.peers.len()
        );
        manager
    }

    pub fn get_peers(&self) -> Vec<PeerId> {
        let peers = self.peers.iter().cloned().collect::<Vec<_>>();
        info!(
            "[PeerManager::get_peers] Returning {} peers: {:?}",
            peers.len(),
            peers
        );
        peers
    }

    pub fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        debug!(
            "[PeerManager::add_peer_with_addr] Adding peer {:?} with addr {:?}",
            peer_id, addr
        );
        if peer_id == self.local_peer_id {
            debug!("[PeerManager::add_peer_with_addr] Skipping self peer");
            return;
        }
        self.peers.insert(peer_id.clone());
        self.peer_addresses.insert(peer_id, addr);
        debug!(
            "[PeerManager::add_peer_with_addr] Current peers: {:?}",
            self.peers
        );
        self.save_to_disk();
    }

    fn save_to_disk(&self) {
        let storage = PeerStorage {
            peers: self
                .peers
                .iter()
                .map(|p| p.to_base58().to_string())
                .collect(),
            addresses: self
                .peer_addresses
                .iter()
                .map(|(p, a)| (p.to_base58().to_string(), a.to_string()))
                .collect(),
        };

        if let Err(e) = storage.save_to_disk(&self.local_peer_id) {
            error!("Failed to save peer storage: {}", e);
        } else {
            info!(
                "Successfully saved peers to disk for {:?}",
                self.local_peer_id
            );
        }
    }

    pub fn get_peer_address(&self, peer_id: &PeerId) -> Option<&Multiaddr> {
        self.peer_addresses.get(peer_id)
    }
}

impl PeerManagement for PeerManager {
    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr) {
        PeerManager::add_peer_with_addr(self, peer_id, addr)
    }

    fn get_peers(&self) -> Vec<PeerId> {
        PeerManager::get_peers(self)
    }
}
