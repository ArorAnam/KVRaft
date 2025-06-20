pub mod types;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::{interval, timeout, Instant, sleep};
use tracing::{debug, info};
use uuid::Uuid;
use rand::Rng;

use crate::{
    error::{KvError, Result},
    storage::{Storage, StorageCommand},
};

use self::types::*;

impl Clone for RaftNode {
    fn clone(&self) -> Self {
        panic!("RaftNode should not be cloned directly, use Arc<RaftNode> instead")
    }
}

pub struct RaftNode {
    pub id: NodeId,
    config: Config,
    state: RwLock<NodeState>,
    current_term: RwLock<Term>,
    voted_for: RwLock<Option<NodeId>>,
    log: RwLock<Vec<LogEntry>>,
    commit_index: RwLock<LogIndex>,
    last_applied: RwLock<LogIndex>,
    leader_id: RwLock<Option<NodeId>>,
    storage: Storage,
    pending_commands: Arc<Mutex<dashmap::DashMap<Uuid, oneshot::Sender<Result<()>>>>>,
    election_timer: Arc<Mutex<Option<Instant>>>,
    client: reqwest::Client,
}

impl RaftNode {
    pub fn new(config: Config, storage: Storage) -> Self {
        Self {
            id: config.node_id,
            config,
            state: RwLock::new(NodeState::Follower),
            current_term: RwLock::new(0),
            voted_for: RwLock::new(None),
            log: RwLock::new(Vec::new()),
            commit_index: RwLock::new(0),
            last_applied: RwLock::new(0),
            leader_id: RwLock::new(None),
            storage,
            pending_commands: Arc::new(Mutex::new(dashmap::DashMap::new())),
            election_timer: Arc::new(Mutex::new(None)),
            client: reqwest::Client::new(),
        }
    }
    
    pub async fn start(self: Arc<Self>) {
        let node = self.clone();
        tokio::spawn(async move {
            node.run_election_timer().await;
        });
        
        let node = self.clone();
        tokio::spawn(async move {
            node.apply_committed_entries().await;
        });
        
        let node = self.clone();
        tokio::spawn(async move {
            node.run_leader_tasks().await;
        });
        
        info!("Raft node {} started", self.id);
    }
    
    async fn run_leader_tasks(&self) {
        loop {
            if self.get_state().await == NodeState::Leader {
                self.leader_loop().await;
            } else {
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
    
    pub async fn get_state(&self) -> NodeState {
        *self.state.read().await
    }
    
    pub async fn get_term(&self) -> Term {
        *self.current_term.read().await
    }
    
    pub async fn get_leader_id(&self) -> Option<NodeId> {
        *self.leader_id.read().await
    }
    
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.storage.get(key))
    }
    
    pub async fn set(&self, key: String, value: String) -> Result<()> {
        let state = self.get_state().await;
        if state != NodeState::Leader {
            let leader_id = self.get_leader_id().await;
            return Err(KvError::NotLeader { leader_id });
        }
        
        let command = StorageCommand {
            key,
            value: Some(value),
        };
        
        let client_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        
        {
            let pending = self.pending_commands.lock().await;
            pending.insert(client_id, tx);
        }
        
        let entry = LogEntry {
            index: self.log.read().await.len() as LogIndex + 1,
            term: self.get_term().await,
            command,
            client_id,
        };
        
        self.log.write().await.push(entry.clone());
        
        self.replicate_log().await;
        
        match timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(KvError::Internal("Command cancelled".to_string())),
            Err(_) => Err(KvError::Timeout),
        }
    }
    
    async fn reset_election_timer(&self) {
        let mut timer = self.election_timer.lock().await;
        *timer = Some(Instant::now());
    }
    
    async fn run_election_timer(&self) {
        let mut interval = interval(Duration::from_millis(10));
        
        loop {
            interval.tick().await;
            
            let state = self.get_state().await;
            if state == NodeState::Leader {
                continue;
            }
            
            let should_start_election = {
                let timer = self.election_timer.lock().await;
                if let Some(last_reset) = *timer {
                    let elapsed = last_reset.elapsed();
                    let timeout = self.random_election_timeout();
                    elapsed >= timeout
                } else {
                    true
                }
            };
            
            if should_start_election {
                self.start_election().await;
            }
        }
    }
    
    fn random_election_timeout(&self) -> Duration {
        let mut rng = rand::thread_rng();
        let timeout_ms = rng.gen_range(
            self.config.election_timeout_min_ms..=self.config.election_timeout_max_ms
        );
        Duration::from_millis(timeout_ms)
    }
    
    async fn start_election(&self) {
        info!("Node {} starting election", self.id);
        
        *self.state.write().await = NodeState::Candidate;
        let mut current_term = self.current_term.write().await;
        *current_term += 1;
        let term = *current_term;
        drop(current_term);
        
        *self.voted_for.write().await = Some(self.id);
        self.reset_election_timer().await;
        
        let log = self.log.read().await;
        let last_log_index = log.len() as LogIndex;
        let last_log_term = log.last().map(|e| e.term).unwrap_or(0);
        drop(log);
        
        let request = RequestVoteRequest {
            term,
            candidate_id: self.id,
            last_log_index,
            last_log_term,
        };
        
        let mut votes = 1; // Vote for self
        let majority = (self.config.peers.len() + 1) / 2 + 1;
        
        // Send vote requests to all peers
        let mut vote_futures = Vec::new();
        for &peer_id in &self.config.peers {
            let request_clone = request.clone();
            let vote_future = self.send_request_vote(peer_id, request_clone);
            vote_futures.push(vote_future);
        }
        
        // Collect votes with timeout
        let vote_results = futures::future::join_all(vote_futures).await;
        
        for vote_result in vote_results {
            if let Some(response) = vote_result {
                if response.term > term {
                    // Found a node with higher term, step down
                    *self.current_term.write().await = response.term;
                    *self.state.write().await = NodeState::Follower;
                    *self.voted_for.write().await = None;
                    return;
                }
                
                if response.vote_granted && response.term == term {
                    votes += 1;
                    if votes >= majority {
                        break;
                    }
                }
            }
        }
        
        // Check if we won the election
        if votes >= majority && self.get_state().await == NodeState::Candidate && self.get_term().await == term {
            self.become_leader().await;
        } else {
            info!("Election failed, got {} votes, needed {}", votes, majority);
        }
    }
    
    async fn become_leader(&self) {
        info!("Node {} became leader for term {}", self.id, self.get_term().await);
        
        *self.state.write().await = NodeState::Leader;
        *self.leader_id.write().await = Some(self.id);
        
        self.send_heartbeats().await;
        
        // Leader loop will be handled by a separate task
    }
    
    async fn leader_loop(&self) {
        let mut interval = interval(Duration::from_millis(self.config.heartbeat_interval_ms));
        
        while self.get_state().await == NodeState::Leader {
            interval.tick().await;
            self.send_heartbeats().await;
        }
    }
    
    async fn send_heartbeats(&self) {
        let term = self.get_term().await;
        let commit_index = *self.commit_index.read().await;
        let log = self.log.read().await;
        let prev_log_index = log.len() as LogIndex;
        let prev_log_term = if prev_log_index > 0 {
            log.get((prev_log_index - 1) as usize).map(|e| e.term).unwrap_or(0)
        } else {
            0
        };
        drop(log);
        
        let request = AppendEntriesRequest {
            term,
            leader_id: self.id,
            prev_log_index,
            prev_log_term,
            entries: vec![],  // Empty for heartbeat
            leader_commit: commit_index,
        };
        
        for &peer_id in &self.config.peers {
            let request_clone = request.clone();
            let peer_addr = self.config.peer_addresses.get(&peer_id).cloned();
            let client = self.client.clone();
            
            tokio::spawn(async move {
                if let Some(addr) = peer_addr {
                    Self::send_append_entries_static(client, addr, request_clone).await;
                }
            });
        }
    }
    
    async fn send_append_entries_static(
        client: reqwest::Client,
        peer_addr: String,
        request: AppendEntriesRequest,
    ) -> Option<AppendEntriesResponse> {
        let url = format!("{}/raft/append_entries", peer_addr);
        
        match client.post(&url)
            .json(&request)
            .timeout(Duration::from_millis(50))
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<AppendEntriesResponse>().await {
                    Ok(append_response) => Some(append_response),
                    Err(e) => {
                        debug!("Failed to parse append response: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                debug!("Failed to send append entries: {}", e);
                None
            }
        }
    }
    
    async fn replicate_log(&self) {
        let commit_index = *self.commit_index.read().await;
        let log = self.log.read().await;
        let new_commit_index = log.len() as LogIndex;
        drop(log);
        
        if new_commit_index > commit_index {
            *self.commit_index.write().await = new_commit_index;
        }
    }
    
    async fn apply_committed_entries(&self) {
        let mut interval = interval(Duration::from_millis(50));
        
        loop {
            interval.tick().await;
            
            let commit_index = *self.commit_index.read().await;
            let mut last_applied = self.last_applied.write().await;
            
            if commit_index > *last_applied {
                let log = self.log.read().await;
                
                for i in (*last_applied as usize)..=(commit_index as usize - 1) {
                    if i < log.len() {
                        let entry = &log[i];
                        self.storage.apply_command(&entry.command);
                        
                        let pending = self.pending_commands.lock().await;
                        if let Some((_, tx)) = pending.remove(&entry.client_id) {
                            let _ = tx.send(Ok(()));
                        }
                    }
                }
                
                *last_applied = commit_index;
            }
        }
    }
    
    pub async fn handle_request_vote(&self, request: RequestVoteRequest) -> RequestVoteResponse {
        let mut current_term = self.current_term.write().await;
        let mut voted_for = self.voted_for.write().await;
        
        // If request term is less than current term, deny vote
        if request.term < *current_term {
            return RequestVoteResponse {
                term: *current_term,
                vote_granted: false,
            };
        }
        
        // If request term is greater than current term, update term and become follower
        if request.term > *current_term {
            *current_term = request.term;
            *voted_for = None;
            *self.state.write().await = NodeState::Follower;
            *self.leader_id.write().await = None;
        }
        
        // Check if we've already voted in this term
        let can_vote = match *voted_for {
            None => true,
            Some(node_id) => node_id == request.candidate_id,
        };
        
        if !can_vote {
            return RequestVoteResponse {
                term: *current_term,
                vote_granted: false,
            };
        }
        
        // Check if candidate's log is at least as up-to-date as ours
        let log = self.log.read().await;
        let our_last_log_index = log.len() as LogIndex;
        let our_last_log_term = log.last().map(|e| e.term).unwrap_or(0);
        
        let log_is_up_to_date = request.last_log_term > our_last_log_term ||
            (request.last_log_term == our_last_log_term && request.last_log_index >= our_last_log_index);
        
        if log_is_up_to_date {
            *voted_for = Some(request.candidate_id);
            self.reset_election_timer().await;
            
            info!("Voted for node {} in term {}", request.candidate_id, request.term);
            
            RequestVoteResponse {
                term: *current_term,
                vote_granted: true,
            }
        } else {
            RequestVoteResponse {
                term: *current_term,
                vote_granted: false,
            }
        }
    }
    
    pub async fn handle_append_entries(&self, request: AppendEntriesRequest) -> AppendEntriesResponse {
        let mut current_term = self.current_term.write().await;
        
        // If request term is less than current term, reject
        if request.term < *current_term {
            return AppendEntriesResponse {
                term: *current_term,
                success: false,
            };
        }
        
        // If request term is greater than current term, update term and become follower
        if request.term > *current_term {
            *current_term = request.term;
            *self.voted_for.write().await = None;
            *self.state.write().await = NodeState::Follower;
        }
        
        // Reset election timer on valid AppendEntries
        self.reset_election_timer().await;
        
        // Update leader ID
        *self.leader_id.write().await = Some(request.leader_id);
        
        // Ensure we're a follower
        *self.state.write().await = NodeState::Follower;
        
        // TODO: Implement log replication logic
        // For now, just accept the heartbeat
        
        AppendEntriesResponse {
            term: *current_term,
            success: true,
        }
    }
    
    async fn send_request_vote(&self, peer_id: NodeId, request: RequestVoteRequest) -> Option<RequestVoteResponse> {
        if let Some(peer_addr) = self.config.peer_addresses.get(&peer_id) {
            let url = format!("{}/raft/request_vote", peer_addr);
            
            match self.client.post(&url)
                .json(&request)
                .timeout(Duration::from_millis(50))
                .send()
                .await
            {
                Ok(response) => {
                    match response.json::<RequestVoteResponse>().await {
                        Ok(vote_response) => Some(vote_response),
                        Err(e) => {
                            debug!("Failed to parse vote response from node {}: {}", peer_id, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to send vote request to node {}: {}", peer_id, e);
                    None
                }
            }
        } else {
            debug!("No address configured for peer {}", peer_id);
            None
        }
    }
}