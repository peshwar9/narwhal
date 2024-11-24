## Narhwal consensus implementation
##### (Note this is for experimental purposes only)

### Design document
[Design Document](./docs/Design.md)

### Quick start

```
# Clone the repository
git clone https://github.com/yourusername/narwhal.git
cd narwhal

# Build the project
cargo build
```

#### Running nodes

```
First node:cargo run -- "" 3001
```


##### Look for log that says something like:
New listen address address=/ip4/192.168.1.74/tcp/60691




### Second node, using the exact P2P address from first node's logs
cargo run -- /ip4/192.168.1.74/tcp/60493 3002

The following will take place:
P2P Connection process should complete successfully:
Noise protocol negotiation should succeed
Yamux transport should be established
Peer ID exchange should be completed
Round-trip time should be  measured (~900 microsecs)

### Send transactions

To first node:
curl -X POST http://localhost:3001/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction 1", "parents": []}'

To second node:
curl -X POST http://localhost:3002/transaction \
  -H "Content-Type: application/json" \
  -d '{"data": "test transaction 2", "parents": []}'

### Query DAG state:

Node 1: curl http://localhost:3001/dag/state 
Node 2: curl http://localhost:3002/dag/state 

### Features
DAG-based transaction dissemination
Peer-to-peer networking using libp2p
HTTP API for transaction submission and state queries
Transaction parent-child relationship validation
Concurrent transaction processing

### Contributing
We welcome contributions to the Narwhal implementation! Here's how you can contribute:
1. First, discuss the change you wish to make via a GitHub issue
2. Fork the repository and create your feature branch
3. Implement your changes with appropriate tests
4. Submit a Pull Request with a comprehensive description of changes

Please ensure your PR:
- Includes relevant tests
- Updates documentation as needed
- Follows the existing code style
- Has a clear and descriptive commit message

### License

MIT License
Copyright (c) 2024 
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
