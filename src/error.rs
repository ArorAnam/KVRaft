use thiserror::Error;

#[derive(Error, Debug)]
pub enum KvError {
    #[error("Key not found")]
    KeyNotFound,
    
    #[error("Not leader")]
    NotLeader { leader_id: Option<u64> },
    
    #[error("Internal error: {0}")]
    Internal(String),
    
    #[error("Timeout")]
    Timeout,
    
    #[error("Network error: {0}")]
    Network(String),
}

pub type Result<T> = std::result::Result<T, KvError>;