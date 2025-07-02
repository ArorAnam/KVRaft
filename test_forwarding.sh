#!/bin/bash

echo "Starting Request Forwarding Test..."

# Kill any existing instances
pkill -f raft-kv-server 2>/dev/null

# Start 3 servers
echo "Starting 3-node cluster..."
./run_cluster.sh

# Wait for cluster to start
sleep 3

# Wait for leader election
echo "Waiting for leader election..."
sleep 5

# Check status of all nodes to find leader
echo "Checking node statuses..."
leader_port=""
follower_ports=()

for port in 3001 3002 3003; do
    status=$(curl -s http://localhost:$port/status)
    state=$(echo $status | jq -r '.state')
    node_id=$(echo $status | jq -r '.node_id')
    echo "Node $node_id on port $port is $state"
    
    if [[ $state == "Leader" ]]; then
        leader_port=$port
    else
        follower_ports+=($port)
    fi
done

if [[ -z "$leader_port" ]]; then
    echo "ERROR: No leader found!"
    exit 1
fi

echo "Leader is on port $leader_port"
echo "Followers are on ports ${follower_ports[@]}"

# Test 1: Write directly to leader
echo -e "\n=== Test 1: Writing directly to leader ==="
curl -X PUT http://localhost:$leader_port/key/test1 \
    -H "Content-Type: application/json" \
    -d '{"value":"direct-to-leader"}' \
    -w "\nHTTP Status: %{http_code}\n"

# Give time for replication
sleep 1

# Test 2: Write to a follower (should be forwarded)
echo -e "\n=== Test 2: Writing to follower (should be forwarded) ==="
follower_port=${follower_ports[0]}
echo "Writing to follower on port $follower_port..."
curl -X PUT http://localhost:$follower_port/key/test2 \
    -H "Content-Type: application/json" \
    -d '{"value":"forwarded-from-follower"}' \
    -w "\nHTTP Status: %{http_code}\n" \
    -v 2>&1 | grep -E "(HTTP/|< |> |{)"

# Give time for replication
sleep 1

# Test 3: Verify data on all nodes
echo -e "\n=== Test 3: Verifying data on all nodes ==="
for port in 3001 3002 3003; do
    echo -e "\nNode on port $port:"
    echo -n "  test1: "
    curl -s http://localhost:$port/key/test1 | jq -r '.value // "NOT FOUND"'
    echo -n "  test2: "
    curl -s http://localhost:$port/key/test2 | jq -r '.value // "NOT FOUND"'
done

# Test 4: Multiple concurrent writes to followers
echo -e "\n=== Test 4: Concurrent writes to different followers ==="
for i in {1..5}; do
    port=${follower_ports[$((i % ${#follower_ports[@]}))]}
    echo "Writing concurrent-$i to follower on port $port..."
    curl -X PUT http://localhost:$port/key/concurrent-$i \
        -H "Content-Type: application/json" \
        -d "{\"value\":\"value-$i\"}" \
        -w " (HTTP %{http_code})\n" &
done

# Wait for all concurrent requests to complete
wait

# Give time for replication
sleep 2

# Verify concurrent writes
echo -e "\n=== Verifying concurrent writes ==="
for i in {1..5}; do
    echo -n "concurrent-$i: "
    curl -s http://localhost:$leader_port/key/concurrent-$i | jq -r '.value // "NOT FOUND"'
done

# Cleanup
echo -e "\n=== Cleaning up ==="
pkill -f raft-kv-server

echo -e "\n=== Request Forwarding Test Complete ==="