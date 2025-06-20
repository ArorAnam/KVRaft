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
        let _last_log_index = log.len() as LogIndex;
        let _last_log_term = log.last().map(|e| e.term).unwrap_or(0);
        drop(log);
        
        let votes = 1;
        let majority = (self.config.peers.len() + 1) / 2 + 1;
        
        for &_peer in &self.config.peers {
            if votes >= majority {
                break;
            }
            
            sleep(Duration::from_millis(10)).await;
        }
        
        if votes >= majority && self.get_state().await == NodeState::Candidate && self.get_term().await == term {
            self.become_leader().await;
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
        let _term = self.get_term().await;
        let _commit_index = *self.commit_index.read().await;
        
        for &peer in &self.config.peers {
            debug!("Sending heartbeat to peer {}", peer);
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
}