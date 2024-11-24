use libp2p::{Multiaddr, PeerId};

pub trait PeerManagement {
    fn get_peers(&self) -> Vec<PeerId>;
    fn add_peer_with_addr(&mut self, peer_id: PeerId, addr: Multiaddr);
}
