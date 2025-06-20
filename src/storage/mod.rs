use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Storage {
    data: Arc<DashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCommand {
    pub key: String,
    pub value: Option<String>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
        }
    }
    
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).map(|v| v.value().clone())
    }
    
    pub fn set(&self, key: String, value: String) {
        self.data.insert(key, value);
    }
    
    pub fn delete(&self, key: &str) -> Option<String> {
        self.data.remove(key).map(|(_, v)| v)
    }
    
    pub fn apply_command(&self, cmd: &StorageCommand) {
        match &cmd.value {
            Some(value) => self.set(cmd.key.clone(), value.clone()),
            None => { self.delete(&cmd.key); },
        }
    }
}