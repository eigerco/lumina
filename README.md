# Lumina

Rust implementation of Celestia's [data availability node](https://github.com/celestiaorg/celestia-node) able to run natively and in browser-based environments.

Run Lumina now at [lumina.rs](https://lumina.rs/) and directly verify Celestia.

Supported features:
- Backward and forward synchronization of block headers within sampling window
- Header exchange (`header-ex`) client and server
- Listening for, verifying and redistributing extended headers on gossip protocol (`header-sub`)
- Listening for, verifying and redistributing fraud proofs on gossip protocol (`fraud-sub`)
- Backward and forward Data Availability Sampling
- Native and browser persistent storage
- Streaming events happening on the node
- Native, wasm and uniffi libraries, embed the node anywhere
- Integration tests with Go implementation

## Installing the node

### Installing with cargo

Install the node. Note that currently to serve lumina to run it from the browser, you need to compile `lumina-cli` manually.
```bash
cargo install lumina-cli --locked
```
Run the node
```bash
lumina node --network mocha
```

### Building from source

Install common dependencies

```bash
# install dependencies
sudo apt-get install -y build-essential curl git

# install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# open a new terminal or run
source "$HOME/.cargo/env"

# clone the repository
git clone https://github.com/eigerco/lumina
cd lumina

# install lumina
cargo install --path cli
```

### Building wasm-node

To build `lumina-cli` with support for serving wasm-node to browsers, currently
you need to compile wasm node manually. Follow these additional steps:

```bash
# install npm and wasm-pack
sudo apt-get install -y npm
cargo install wasm-pack

# compile lumina to wasm
wasm-pack build node-wasm

# build the local webpage
cd cli/js
npm i && npm run build
cd -

# install lumina-cli
cargo install --path cli --features browser-node
```

## Running the node

### Running the node natively

```bash
# run lumina node
lumina node --network mocha

# check out help for more configuration options
lumina node --help
```

### Serving node-wasm

```bash
# serve lumina node on default localhost:9876
lumina browser

# check out help from more configuration options
lumina browser --help
```

#### WebTransport and Secure Contexts

For security reasons, browsers only allow WebTransport to be used in [Secure Context](https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts). When running Lumina in a browser make sure to access it either locally or over HTTPS.

## Running Go Celestia node for integration

Follow [this guide](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry#authenticating-with-a-personal-access-token-classic)
to authorize yourself in github's container registry.

Starting a Celestia network with single validator and bridge
```bash
docker compose -f ci/docker-compose.yml up --build --force-recreate -d
# and to stop it
docker compose -f ci/docker-compose.yml down
```
> **Note:**
> You can run more bridge nodes by uncommenting/copying the bridge service definition in `ci/docker-compose.yml`.

To get a JWT token for a topped up account (coins will be transferred in block 2):
```bash
export CELESTIA_NODE_AUTH_TOKEN=$(docker compose -f ci/docker-compose.yml exec bridge-0 celestia bridge auth admin --p2p.network private)
```

Accessing json RPC api with Go `celestia` cli:
```bash
docker compose -f ci/docker-compose.yml exec bridge-0 \
    celestia blob submit 0x0c204d39600fddd3 '"Hello world"' --token "$CELESTIA_NODE_AUTH_TOKEN"
```

Extracting blocks for test cases:
```bash
docker compose -f ci/docker-compose.yml exec bridge-0 \
    celestia header get-by-height 27 --token "$CELESTIA_NODE_AUTH_TOKEN" | jq .result
```

## Running integration tests with Celestia node

Make sure you have the Celestia network running inside docker compose from the section above.

Generate authentication tokens
```bash
./tools/gen_auth_tokens.sh
```

Run tests
```bash
cargo test
```

## Upgrading dependencies

Some of our users use `celestia-types` with [risc0](https://github.com/risc0)
zkVM, which applies some acceleration on dependencies related to cryptography.
Such dependency is `sha2`.

Because of that we created `./tools/upgrade-deps.sh` script which upgrades all
dependencies in `Cargo.toml` except the ones that are patched by risc0.

How to upgrade:

```bash
./tools/upgrade-deps.sh -i  # `-i` upgrades incompatible versions too
cargo update
```

## Frontend

Check out the front end at [eigerco/lumina-front](https://github.com/eigerco/lumina-front)

## About Eiger

We are engineers. We contribute to various ecosystems by building low level implementations and core components. We built Lumina because we believe in the modular thesis. We wanted to make the Celestia light node available and easy to run for as many users so that everyone can perform sampling to ensure data availability.

Contact us at hello@eiger.co
