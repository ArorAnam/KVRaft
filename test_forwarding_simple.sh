#!/bin/bash

echo "Building the project..."
cargo build --release --quiet

echo "Starting nodes..."
# Start nodes in background without logs
RUST_LOG=error ./target/release/raft-kv-server --node-id 1 --port 3001 --peers 2:http://127.0.0.1:3002,3:http://127.0.0.1:3003 > /dev/null 2>&1 &
P1=$!
RUST_LOG=error ./target/release/raft-kv-server --node-id 2 --port 3002 --peers 1:http://127.0.0.1:3001,3:http://127.0.0.1:3003 > /dev/null 2>&1 &
P2=$!
RUST_LOG=error ./target/release/raft-kv-server --node-id 3 --port 3003 --peers 1:http://127.0.0.1:3001,2:http://127.0.0.1:3002 > /dev/null 2>&1 &
P3=$!

echo "Waiting for leader election..."
sleep 5

# Find leader
echo "Finding leader..."
leader_port=""
follower_port=""

for port in 3001 3002 3003; do
    status=$(curl -s http://localhost:$port/status)
    state=$(echo $status | jq -r '.state' 2>/dev/null)
    if [[ $state == "Leader" ]]; then
        leader_port=$port
    elif [[ $state == "Follower" ]] && [[ -z $follower_port ]]; then
        follower_port=$port
    fi
done

echo "Leader: port $leader_port"
echo "Follower: port $follower_port"

# Test 1: Direct write to leader
echo -e "\nTest 1: Writing to leader..."
curl -s -X PUT http://localhost:$leader_port/key/test1 \
    -H "Content-Type: application/json" \
    -d '{"value":"direct"}' \
    -w "Status: %{http_code}\n"

# Test 2: Write to follower (should forward)
echo -e "\nTest 2: Writing to follower..."
curl -s -X PUT http://localhost:$follower_port/key/test2 \
    -H "Content-Type: application/json" \
    -d '{"value":"forwarded"}' \
    -w "Status: %{http_code}\n"

# Give time for replication
sleep 1

# Verify
echo -e "\nVerifying data..."
echo -n "test1 on leader: "
curl -s http://localhost:$leader_port/key/test1 | jq -r '.value // "NOT FOUND"'
echo -n "test2 on leader: "
curl -s http://localhost:$leader_port/key/test2 | jq -r '.value // "NOT FOUND"'

# Cleanup
kill $P1 $P2 $P3 2>/dev/null
echo "Done"