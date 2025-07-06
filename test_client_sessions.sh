#!/bin/bash

echo "Testing Client Session Management and Request Deduplication..."

# Build the project
echo "Building the project..."
cargo build --release --quiet

# Start nodes
echo "Starting nodes..."
RUST_LOG=info ./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2,3 > node1.log 2>&1 &
P1=$!
RUST_LOG=info ./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1,3 > node2.log 2>&1 &
P2=$!
RUST_LOG=info ./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1,2 > node3.log 2>&1 &
P3=$!

echo "Waiting for leader election..."
sleep 5

# Find leader
echo "Finding leader..."
leader_port=""
for port in 3001 3002 3003; do
    status=$(curl -s http://localhost:$port/status)
    state=$(echo $status | jq -r '.state' 2>/dev/null)
    if [[ $state == "Leader" ]]; then
        leader_port=$port
        break
    fi
done

if [[ -z "$leader_port" ]]; then
    echo "ERROR: No leader found!"
    kill $P1 $P2 $P3 2>/dev/null
    exit 1
fi

echo "Leader found on port $leader_port"

# Test 1: Request without client session (backward compatibility)
echo -e "\n=== Test 1: Request without client session ==="
response=$(curl -s -X PUT http://localhost:$leader_port/key/test1 \
    -H "Content-Type: application/json" \
    -d '{"value":"no-session"}')
echo "Response: $response"

# Test 2: Request with client session
echo -e "\n=== Test 2: Request with client session ==="
response=$(curl -s -X PUT http://localhost:$leader_port/key/test2 \
    -H "Content-Type: application/json" \
    -d '{"value":"with-session","client_id":123,"request_id":1}')
echo "Response: $response"

# Test 3: Duplicate request (should be deduplicated)
echo -e "\n=== Test 3: Duplicate request (should be deduplicated) ==="
sleep 1  # Allow first request to complete
response=$(curl -s -X PUT http://localhost:$leader_port/key/test2 \
    -H "Content-Type: application/json" \
    -d '{"value":"different-value","client_id":123,"request_id":1}')
echo "Response: $response"

# Test 4: Same client, different request ID
echo -e "\n=== Test 4: Same client, different request ID ==="
response=$(curl -s -X PUT http://localhost:$leader_port/key/test3 \
    -H "Content-Type: application/json" \
    -d '{"value":"new-request","client_id":123,"request_id":2}')
echo "Response: $response"

# Test 5: Different client, same request ID
echo -e "\n=== Test 5: Different client, same request ID ==="
response=$(curl -s -X PUT http://localhost:$leader_port/key/test4 \
    -H "Content-Type: application/json" \
    -d '{"value":"different-client","client_id":456,"request_id":1}')
echo "Response: $response"

# Give time for replication
sleep 2

# Verify values
echo -e "\n=== Verifying stored values ==="
for key in test1 test2 test3 test4; do
    value=$(curl -s http://localhost:$leader_port/key/$key | jq -r '.value // "NOT FOUND"')
    echo "$key: $value"
done

# Check logs for deduplication messages
echo -e "\n=== Checking for deduplication logs ==="
if grep -q "Duplicate request detected" node*.log; then
    echo "✓ Deduplication detected in logs"
    grep "Duplicate request detected" node*.log | head -5
else
    echo "✗ No deduplication messages found"
fi

# Cleanup
echo -e "\n=== Cleaning up ==="
kill $P1 $P2 $P3 2>/dev/null
rm -f node*.log

echo "Client session testing complete!"