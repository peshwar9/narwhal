use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: String,
    pub data: String,
    pub parents: Vec<String>,
}

impl Transaction {
    pub fn new(data: String, parents: Vec<String>) -> Self {
        // Create a unique ID by hashing the data and parents
        let mut hasher = Sha256::new();
        hasher.update(&data);
        for parent in &parents {
            hasher.update(parent);
        }
        let id = format!("{:x}", hasher.finalize());

        Transaction { id, data, parents }
    }
}
