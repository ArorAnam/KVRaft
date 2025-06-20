#!/bin/bash

echo "Building the project..."
cargo build --release

echo "Starting 3-node Raft cluster..."

# Start node 1
echo "Starting node 1 on port 3001..."
./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2,3 &
NODE1_PID=$!

# Start node 2
echo "Starting node 2 on port 3002..."
./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1,3 &
NODE2_PID=$!

# Start node 3
echo "Starting node 3 on port 3003..."
./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1,2 &
NODE3_PID=$!

echo "Cluster started with PIDs: $NODE1_PID, $NODE2_PID, $NODE3_PID"
echo "Press Ctrl+C to stop all nodes"

# Function to cleanup on exit
cleanup() {
    echo "Stopping all nodes..."
    kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null
    exit
}

# Set up trap to cleanup on Ctrl+C
trap cleanup INT

# Wait for all background processes
wait