use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::storage::StorageCommand;

pub type Term = u64;
pub type LogIndex = u64;
pub type NodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub command: StorageCommand,
    pub client_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    pub term: Term,
    pub leader_id: NodeId,
    pub prev_log_index: LogIndex,
    pub prev_log_term: Term,
    pub entries: Vec<LogEntry>,
    pub leader_commit: LogIndex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: Term,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteRequest {
    pub term: Term,
    pub candidate_id: NodeId,
    pub last_log_index: LogIndex,
    pub last_log_term: Term,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteResponse {
    pub term: Term,
    pub vote_granted: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub node_id: NodeId,
    pub peers: Vec<NodeId>,
    pub peer_addresses: HashMap<NodeId, String>,
    pub election_timeout_min_ms: u64,
    pub election_timeout_max_ms: u64,
    pub heartbeat_interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_id: 1,
            peers: vec![],
            peer_addresses: HashMap::new(),
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
        }
    }
}