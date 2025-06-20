mod api;
mod error;
mod raft;
mod storage;

use std::sync::Arc;
use std::collections::HashMap;
use clap::Parser;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::{
    api::{create_router, ApiState},
    raft::{RaftNode, types::Config},
    storage::Storage,
};

#[derive(Parser, Debug)]
#[command(name = "raft-kv-server")]
#[command(about = "Distributed KV store using Raft consensus")]
struct Args {
    #[arg(short, long, default_value = "1")]
    node_id: u64,
    
    #[arg(short, long, default_value = "3000")]
    port: u16,
    
    #[arg(short = 'c', long, value_delimiter = ',')]
    peers: Vec<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");
    
    let args = Args::parse();
    
    info!("Starting Raft KV server node {} on port {}", args.node_id, args.port);
    info!("Peers: {:?}", args.peers);
    
    // Build peer addresses map
    let mut peer_addresses = HashMap::new();
    for &peer_id in &args.peers {
        // Assuming peers are on sequential ports starting from 3001
        let peer_port = 3000 + peer_id;
        peer_addresses.insert(peer_id, format!("http://127.0.0.1:{}", peer_port));
    }
    
    let config = Config {
        node_id: args.node_id,
        peers: args.peers,
        peer_addresses,
        ..Default::default()
    };
    
    let storage = Storage::new();
    let raft_node = Arc::new(RaftNode::new(config, storage));
    
    raft_node.clone().start().await;
    
    let api_state = ApiState {
        raft_node: raft_node.clone(),
    };
    
    let app = create_router(api_state);
    
    let addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    info!("Server listening on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
