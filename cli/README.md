# lumina-cli

Command line interface for running [Lumina](../README.md) node for the Celestia network either locally or in a browser.

When built with default features, lumina-cli compiles only natively running code. If you want to serve lumina-wasm-node and run it in browser, use `embedded-lumina` feature flag.
As a shorthand, `lumina` executable can be renamed to `lumina-node`, which will cause it to act as local node only, same as if invoked with `lumina node`.


## Building and running

Install common dependencies

```bash
# install dependencies
sudo apt-get install -y build-essential curl git protobuf-compiler

# install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# open a new terminal or run
source "$HOME/.cargo/env"

# clone the repository
git clone https://github.com/eigerco/lumina
cd lumina
```

### Running the node natively

```bash
# install lumina
cargo install --path cli

# run lumina node
lumina node --network mocha

# check out help for more configuration options
lumina node --help
```

### Building and serving node-wasm

```bash
# build wasm-node to be bundled with lumina
wasm-pack build --target web node-wasm

# install lumina with wasm node embedded
cargo install --path cli --features embedded-lumina

# serve lumina node on default localhost:9876
lumina browser
```
