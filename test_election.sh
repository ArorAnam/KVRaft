#!/bin/bash

echo "Testing Raft leader election..."
echo

# Build the project
echo "Building..."
cargo build --release

# Start the cluster with modified scripts
echo "Starting 3-node cluster..."

# Kill any existing processes on these ports
pkill -f "raft-kv-server" 2>/dev/null
sleep 1

# Start node 1
echo "Starting node 1 on port 3001..."
RUST_LOG=info ./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2,3 > node1.log 2>&1 &
NODE1_PID=$!

# Start node 2
echo "Starting node 2 on port 3002..."
RUST_LOG=info ./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1,3 > node2.log 2>&1 &
NODE2_PID=$!

# Start node 3
echo "Starting node 3 on port 3003..."
RUST_LOG=info ./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1,2 > node3.log 2>&1 &
NODE3_PID=$!

echo "Cluster started. Waiting for leader election..."
sleep 5

# Check status of all nodes
echo
echo "=== Node Status After Election ==="
for port in 3001 3002 3003; do
    echo "Node on port $port:"
    curl -s "http://127.0.0.1:$port/status" | jq '.' 2>/dev/null || echo "Failed to get status"
    echo
done

# Test write to non-leader (should redirect)
echo "=== Testing Write Operations ==="
echo "Attempting to write to each node..."
for port in 3001 3002 3003; do
    echo -n "Writing to node on port $port: "
    response=$(curl -s -X PUT "http://127.0.0.1:$port/key/test-key" \
        -H "Content-Type: application/json" \
        -d '{"value":"test-value"}' -w "\nHTTP_STATUS:%{http_code}")
    
    http_status=$(echo "$response" | grep HTTP_STATUS | cut -d: -f2)
    if [ "$http_status" = "200" ]; then
        echo "SUCCESS (This is the leader)"
    else
        echo "FAILED with status $http_status (Not leader)"
        echo "$response" | grep -v HTTP_STATUS | jq '.' 2>/dev/null
    fi
done

echo
echo "=== Checking Logs ==="
echo "Last 5 lines of each node log:"
for i in 1 2 3; do
    echo "Node $i:"
    tail -5 "node$i.log" | grep -E "(leader|election|vote|term)"
    echo
done

# Cleanup
echo "Cleaning up..."
kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null
rm -f node*.log

echo "Test complete!"