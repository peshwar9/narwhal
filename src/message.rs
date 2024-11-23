use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionMessage {
    pub transaction_id: String,   // Unique identifier for the transaction
    pub transaction_data: String, // Data associated with the transaction (e.g., transaction details)
    pub parents: Vec<String>, // List of parent transaction IDs in the DAG (can be empty for the first transaction)
}
