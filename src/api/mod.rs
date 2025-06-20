use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::{
    error::{KvError, Result},
    raft::{RaftNode, types::{RequestVoteRequest, RequestVoteResponse, AppendEntriesRequest, AppendEntriesResponse}},
};

#[derive(Clone)]
pub struct ApiState {
    pub raft_node: Arc<RaftNode>,
}

#[derive(Serialize, Deserialize)]
pub struct GetResponse {
    pub value: String,
}

#[derive(Serialize, Deserialize)]
pub struct PutRequest {
    pub value: String,
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
    
    state.raft_node.set(key, payload.value).await?;
    Ok(StatusCode::OK)
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