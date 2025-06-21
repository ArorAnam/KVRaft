#!/bin/bash

echo "Testing Raft log replication..."
echo

# Build the project
echo "Building..."
cargo build --release

# Kill any existing processes
pkill -f "raft-kv-server" 2>/dev/null
sleep 1

# Start the cluster
echo "Starting 3-node cluster..."

# Start nodes with detailed logging
RUST_LOG=raft_kv=debug ./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2,3 > node1.log 2>&1 &
NODE1_PID=$!

RUST_LOG=raft_kv=debug ./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1,3 > node2.log 2>&1 &
NODE2_PID=$!

RUST_LOG=raft_kv=debug ./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1,2 > node3.log 2>&1 &
NODE3_PID=$!

echo "Waiting for leader election..."
sleep 5

CLIENT="./target/release/raft-kv-client"

# Find the leader
echo "=== Finding Leader ==="
LEADER_PORT=""
for port in 3001 3002 3003; do
    status=$(curl -s "http://127.0.0.1:$port/status" | jq -r '.state' 2>/dev/null)
    if [ "$status" = "Leader" ]; then
        LEADER_PORT=$port
        echo "Leader found on port $port"
        break
    fi
done

if [ -z "$LEADER_PORT" ]; then
    echo "No leader found! Check logs."
    tail -20 node*.log
    kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null
    exit 1
fi

# Test replication
echo
echo "=== Testing Replication ==="
echo "Writing 5 key-value pairs to the leader..."
for i in 1 2 3 4 5; do
    echo "Writing key$i=value$i"
    $CLIENT --server "http://127.0.0.1:$LEADER_PORT" put "key$i" "value$i"
done

# Wait for replication
echo
echo "Waiting for replication to complete..."
sleep 2

# Verify replication on all nodes
echo
echo "=== Verifying Replication ==="
for port in 3001 3002 3003; do
    echo "Node on port $port:"
    for i in 1 2 3 4 5; do
        printf "  key%d: " "$i"
        $CLIENT --server "http://127.0.0.1:$port" get "key$i" 2>&1 || echo "NOT FOUND"
    done
    echo
done

# Check logs for replication activity
echo "=== Replication Log Summary ==="
echo "Node 1 log entries:"
grep -E "(Appended|commit|replicate)" node1.log | tail -5

echo
echo "Node 2 log entries:"
grep -E "(Appended|commit|replicate)" node2.log | tail -5

echo
echo "Node 3 log entries:"
grep -E "(Appended|commit|replicate)" node3.log | tail -5

# Cleanup
echo
echo "Cleaning up..."
kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null
rm -f node*.log

echo "Test complete!"