# Raft-based Distributed Key-Value Store

A distributed key-value store implementation in Rust using the Raft consensus algorithm.

## Features

- Raft consensus for distributed consistency
- Leader election and log replication
- HTTP REST API for client interactions
- In-memory key-value storage
- CLI client for testing
- Multi-node cluster support

## Building

```bash
cargo build --release
```

## Running a Cluster

### Option 1: Use the cluster script

```bash
./run_cluster.sh
```

This starts a 3-node cluster on ports 3001, 3002, and 3003.

### Option 2: Start nodes manually

```bash
# Node 1
./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2,3

# Node 2
./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1,3

# Node 3
./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1,2
```

## Using the CLI Client

### Write a value
```bash
./target/release/raft-kv-client --server http://127.0.0.1:3001 put mykey myvalue
```

### Read a value
```bash
./target/release/raft-kv-client --server http://127.0.0.1:3001 get mykey
```

### Check node status
```bash
./target/release/raft-kv-client --server http://127.0.0.1:3001 status
```

## Testing

Run the test script to see the cluster in action:

```bash
./test_cluster.sh
```

## API Endpoints

- `GET /key/{key}` - Get a value
- `PUT /key/{key}` - Set a value (JSON body: `{"value": "..."}`)
- `GET /status` - Get node status

## Architecture

The system consists of:
- **Raft Module**: Implements leader election and log replication
- **Storage Module**: In-memory key-value store
- **API Module**: HTTP REST endpoints
- **Client**: CLI tool for interacting with the cluster

## Quick Start

1. Build the project:
   ```bash
   cargo build --release
   ```

2. Start a 3-node cluster:
   ```bash
   ./run_cluster.sh
   ```

3. In another terminal, test the cluster:
   ```bash
   # Write a value
   ./target/release/raft-kv-client --server http://127.0.0.1:3001 put mykey myvalue
   
   # Read the value
   ./target/release/raft-kv-client --server http://127.0.0.1:3001 get mykey
   
   # Check node status
   ./target/release/raft-kv-client --server http://127.0.0.1:3001 status
   ```

## Current Limitations

This is a basic implementation for educational purposes. Production features not included:
- Persistent storage
- Log compaction
- Membership changes
- Snapshots
- Network partitions handling
- Full Raft protocol (missing proper leader election voting, log replication to followers)