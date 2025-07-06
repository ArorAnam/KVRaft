use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, debug};

use crate::{
    error::{KvError, Result},
    raft::{RaftNode, types::{RequestVoteRequest, RequestVoteResponse, AppendEntriesRequest, AppendEntriesResponse, NodeState, ClientId, RequestId}},
};

#[derive(Clone)]
pub struct ApiState {
    pub raft_node: Arc<RaftNode>,
}

impl ApiState {
    async fn forward_to_leader(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<axum::response::Response> {
        let leader_id = self.raft_node.get_leader_id().await
            .ok_or_else(|| KvError::NoLeaderElected)?;
        
        debug!("Forwarding request to leader {}", leader_id);
        
        let leader_address = self.raft_node.get_peer_address(leader_id)
            .ok_or_else(|| KvError::PeerNotFound(leader_id))?;
        
        let url = format!("{}{}", leader_address, path);
        info!("Forwarding {} request to {}", method, url);
        let client = reqwest::Client::new();
        
        let mut request = match method {
            "GET" => client.get(&url),
            "PUT" => client.put(&url),
            _ => return Err(KvError::Internal("Unsupported method".to_string())),
        };
        
        if let Some(body) = body {
            request = request.json(&body);
        }
        
        let response = request.send().await
            .map_err(|e| KvError::Network(e.to_string()))?;
        
        let status = StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        
        let body = response.bytes().await
            .map_err(|e| KvError::Network(e.to_string()))?;
        
        Ok((status, body).into_response())
    }
}

#[derive(Serialize, Deserialize)]
pub struct GetResponse {
    pub value: String,
}

#[derive(Serialize, Deserialize)]
pub struct PutRequest {
    pub value: String,
    pub client_id: Option<ClientId>,
    pub request_id: Option<RequestId>,
}

#[derive(Serialize, Deserialize)]
pub struct PutResponse {
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub leader_id: Option<u64>,
}

pub fn create_router(state: ApiState) -> Router {
    Router::new()
        .route("/key/:key", get(get_handler))
        .route("/key/:key", put(put_handler))
        .route("/status", get(status_handler))
        .route("/raft/request_vote", post(request_vote_handler))
        .route("/raft/append_entries", post(append_entries_handler))
        .with_state(state)
}

async fn get_handler(
    Path(key): Path<String>,
    State(state): State<ApiState>,
) -> Result<impl IntoResponse> {
    info!("GET request for key: {}", key);
    
    match state.raft_node.get(&key).await {
        Ok(Some(value)) => Ok(Json(GetResponse { value })),
        Ok(None) => Err(KvError::KeyNotFound),
        Err(e) => Err(e),
    }
}

async fn put_handler(
    Path(key): Path<String>,
    State(state): State<ApiState>,
    Json(payload): Json<PutRequest>,
) -> Result<impl IntoResponse> {
    info!("PUT request for key: {} value: {}", key, payload.value);
    
    // Check if we're the leader
    let current_state = state.raft_node.get_state().await;
    debug!("Current node state: {:?}", current_state);
    
    if current_state != NodeState::Leader {
        // Forward to leader
        info!("Node is not leader, forwarding request to leader");
        let path = format!("/key/{}", key);
        let body = serde_json::json!({ 
            "value": payload.value,
            "client_id": payload.client_id,
            "request_id": payload.request_id
        });
        return state.forward_to_leader("PUT", &path, Some(body)).await;
    }
    
    // Use the enhanced set method with client session support
    state.raft_node.set_with_session(key, payload.value, payload.client_id, payload.request_id).await?;
    
    let response = PutResponse {
        success: true,
        message: Some("Value successfully stored".to_string()),
    };
    Ok(Json(response).into_response())
}

async fn status_handler(State(state): State<ApiState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "node_id": state.raft_node.id,
        "state": format!("{:?}", state.raft_node.get_state().await),
        "term": state.raft_node.get_term().await,
        "leader_id": state.raft_node.get_leader_id().await,
    }))
}

impl IntoResponse for KvError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_response) = match self {
            KvError::KeyNotFound => (
                StatusCode::NOT_FOUND,
                ErrorResponse {
                    error: self.to_string(),
                    leader_id: None,
                },
            ),
            KvError::NotLeader { leader_id } => (
                StatusCode::MISDIRECTED_REQUEST,
                ErrorResponse {
                    error: self.to_string(),
                    leader_id,
                },
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse {
                    error: self.to_string(),
                    leader_id: None,
                },
            ),
        };
        
        (status, Json(error_response)).into_response()
    }
}

async fn request_vote_handler(
    State(state): State<ApiState>,
    Json(request): Json<RequestVoteRequest>,
) -> impl IntoResponse {
    info!("Received RequestVote RPC from node {}", request.candidate_id);
    let response = state.raft_node.handle_request_vote(request).await;
    Json(response)
}

async fn append_entries_handler(
    State(state): State<ApiState>,
    Json(request): Json<AppendEntriesRequest>,
) -> impl IntoResponse {
    info!("Received AppendEntries RPC from leader {}", request.leader_id);
    let response = state.raft_node.handle_append_entries(request).await;
    Json(response)
}