use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: String,
    pub data: String,
    pub parents: Vec<String>, // References to parent transactions
}
// The ID cryptographically links the transaction to its data and parents ensuring uniqueness, tamper-proofness, and integrity (key DAG property)
impl Transaction {
    pub fn new(data: String, parents: Vec<String>) -> Self {
        let mut hasher = Sha256::new(); // create a new SHA256 hasher
        hasher.update(&data); // Add the transaction data to the hasher
        for parent in &parents {
            hasher.update(&parent); // add each parent's ID to the hasher
        }
        let id = format!("{:x}", hasher.finalize()); // Compute the final hash ( 256-bit value) and format it as a hex string, which is stored in id
        Transaction { id, data, parents } // Create and return the transaction instance
    }
}
