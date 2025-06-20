#!/bin/bash

echo "Testing Raft KV cluster..."
echo

# Build the client
echo "Building client..."
cargo build --release --bin raft-kv-client

CLIENT="./target/release/raft-kv-client"

# Wait for cluster to start
echo "Waiting for cluster to initialize..."
sleep 3

# Test status on all nodes
echo "=== Checking node status ==="
for port in 3001 3002 3003; do
    echo "Node on port $port:"
    $CLIENT --server "http://127.0.0.1:$port" status
    echo
done

# Test write operations
echo "=== Testing write operations ==="
echo "Writing key1=value1 to node 1..."
$CLIENT --server "http://127.0.0.1:3001" put key1 value1

echo "Writing key2=value2 to node 2..."
$CLIENT --server "http://127.0.0.1:3002" put key2 value2

echo "Writing key3=value3 to node 3..."
$CLIENT --server "http://127.0.0.1:3003" put key3 value3

# Wait for replication
sleep 1

# Test read operations
echo
echo "=== Testing read operations ==="
for key in key1 key2 key3; do
    echo "Reading $key from all nodes:"
    for port in 3001 3002 3003; do
        echo -n "  Node $port: "
        $CLIENT --server "http://127.0.0.1:$port" get $key
    done
    echo
done

echo "Test completed!"