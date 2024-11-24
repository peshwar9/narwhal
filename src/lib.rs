pub mod traits;
pub mod p2p;
pub mod behavior;
pub mod dag;
pub mod message;
pub mod transaction;


// Add these explicit re-exports
pub use crate::p2p::PeerManager;
pub use crate::behavior::Behavior;

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

pub type SharedPeerManager = Arc<TokioMutex<crate::p2p::PeerManager>>;