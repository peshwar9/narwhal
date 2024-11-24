use crate::transaction::Transaction;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub struct DAG {
    pub transactions: HashMap<String, Transaction>,
}

impl DAG {
    pub fn new() -> Self {
        DAG {
            transactions: HashMap::new(),
        }
    }

    pub fn add_transaction(&mut self, txn: Transaction) {
        if self.validate_parents(&txn.parents) {
            self.transactions.insert(txn.id.clone(), txn);
        } else {
            println!("Invalid parents for transaction: {}", txn.id);
        }
    }

    pub fn validate_parents(&self, parents: &[String]) -> bool {
        parents
            .iter()
            .all(|parent_id| self.transactions.contains_key(parent_id))
    }

    pub fn print_dag(&self) {
        for (id, txn) in &self.transactions {
            println!(
                "Transaction ID: {}, Data: {}, Parents: {:?}",
                id, txn.data, txn.parents
            );
        }
    }

    pub fn get_all_transactions(&self) -> Vec<&Transaction> {
        self.transactions.values().collect()
    }
}

impl Default for DAG {
    fn default() -> Self {
        Self::new()
    }
}

impl Transaction {
    pub fn data(&self) -> &str {
        &self.data
    }

    pub fn parents(&self) -> &Vec<String> {
        &self.parents
    }

    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        hasher.update(&self.id);
        format!("{:x}", hasher.finalize())
    }
}
