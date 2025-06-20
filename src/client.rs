use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "raft-kv-client")]
#[command(about = "CLI client for Raft KV store")]
struct Args {
    #[arg(short, long, default_value = "http://127.0.0.1:3000")]
    server: String,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Get {
        key: String,
    },
    Put {
        key: String,
        value: String,
    },
    Status,
}

#[derive(Serialize, Deserialize)]
struct GetResponse {
    value: String,
}

#[derive(Serialize, Deserialize)]
struct PutRequest {
    value: String,
}

#[derive(Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
    leader_id: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::new();
    
    match args.command {
        Commands::Get { key } => {
            let url = format!("{}/key/{}", args.server, key);
            let response = client.get(&url).send().await?;
            
            if response.status().is_success() {
                let data: GetResponse = response.json().await?;
                println!("{}", data.value);
            } else {
                let error: ErrorResponse = response.json().await?;
                eprintln!("Error: {}", error.error);
                if let Some(leader_id) = error.leader_id {
                    eprintln!("Leader ID: {}", leader_id);
                }
                std::process::exit(1);
            }
        }
        
        Commands::Put { key, value } => {
            let url = format!("{}/key/{}", args.server, key);
            let request = PutRequest { value };
            let response = client.put(&url).json(&request).send().await?;
            
            if response.status().is_success() {
                println!("OK");
            } else {
                let error: ErrorResponse = response.json().await?;
                eprintln!("Error: {}", error.error);
                if let Some(leader_id) = error.leader_id {
                    eprintln!("Leader ID: {}", leader_id);
                }
                std::process::exit(1);
            }
        }
        
        Commands::Status => {
            let url = format!("{}/status", args.server);
            let response = client.get(&url).send().await?;
            
            if response.status().is_success() {
                let data: serde_json::Value = response.json().await?;
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                eprintln!("Failed to get status");
                std::process::exit(1);
            }
        }
    }
    
    Ok(())
}