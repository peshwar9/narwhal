use crate::transaction::Transaction;
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
}
