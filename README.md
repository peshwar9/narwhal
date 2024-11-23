Run first server with : cargo run -- "" 3001


# Second node, using the exact P2P address from first node's logs
cargo run -- /ip4/192.168.1.74/tcp/60493 3002


# Send transactions

To first node:
curl -X POST http://localhost:3001/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction", "parents": []}'

To second node:
curl -X POST http://localhost:3002/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction", "parents": []}'
