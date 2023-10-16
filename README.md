# Rust Celestia node

Rust implementation of Celestia's [data availability node](https://github.com/celestiaorg/celestia-node) able to run natively and in the browser-based environments.

Supported features:
- [x] Synchronize and verify `ExtendedHeader`s from genesis to the network head
- [x] Header exchange (`header-ex`) client and server
- [x] Listening for, verifying and redistributing extended headers on gossip protocol (`header-sub`)
- [x] Persistent store for Headers
- [x] Integration tests with Go implementation
- [ ] Data Availability Sampling
- [ ] Creating, distributing, and listening for Fraud proofs

## Building and running celestia-node

```
# install dependencies
sudo apt-get install -y build-essential curl git protobuf-compiler

# install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# open a new terminal or run
source "$HOME/.cargo/env"

# clone the repository
git clone https://github.com/eigerco/celestia-node-rs
cd celestia-node-rs

# install celestia-node
cargo install --path celestia

# run celestia node
celestia-node --network mocha

# check out help for more configuration options
celestia-node --help
```

## Running Go celestia node for integration

Follow [this guide](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry#authenticating-with-a-personal-access-token-classic)
to authorize yourself in github's container registry.

Start a celestia network with single validator and bridge
```bash
docker-compose -f ci/docker-compose.yml up --build --force-recreate -d
```

When deleting, remember to also delete subvolumes:
```bash
docker-compose -f ci/docker-compose.yml down -v
```

To get the JWT token for the account with coins (coins will be transferred in block 2):
```bash
export CELESTIA_NODE_AUTH_TOKEN=$(docker-compose -f ci/docker-compose.yml exec bridge celestia bridge auth admin --p2p.network private)
```

Accessing json RPC api with Go `celestia` cli:
```bash
celestia rpc blob Submit 0x0c204d39600fddd3 '"Hello world"' --print-request
```

Extracting blocks for test cases:
```bash
celestia rpc header GetByHeight 27 | jq .result
```

## Running integration tests with celestia node

Make sure you have the celestia network running inside docker-compose from the section above.

Generate authentication tokens for the tests
```
./tools/gen_auth_tokens.sh
```

Run tests
```
cargo test
```
