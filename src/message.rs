use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GreetRequest {
    pub message: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GreetResponse {
    pub message: String
}