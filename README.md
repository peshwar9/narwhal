Run first server with : cargo run -- "" 3001

Look for log that says something like:
New listen address address=/ip4/192.168.1.74/tcp/60691




# Second node, using the exact P2P address from first node's logs
cargo run -- /ip4/192.168.1.74/tcp/60493 3002

The following will take place:
P2P Connection process should complete successfully:
Noise protocol negotiation should succeed
Yamux transport should be established
Peer ID exchange should be completed
Round-trip time should be  measured (~900 microsecs)

# Send transactions

To first node:
curl -X POST http://localhost:3001/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction", "parents": []}'

To second node:
curl -X POST http://localhost:3002/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction", "parents": []}'

Query DAG state:

Node 1: curl http://localhost:3001/dag/state 
Node 2: curl http://localhost:3002/dag/state 