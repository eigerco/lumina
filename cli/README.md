# lumina-cli

Command line interface for running [Lumina](../README.md) node for the Celestia network either locally or in a browser.

When built with default features, lumina-cli compiles only natively running code. If you want to serve lumina-wasm-node and run it in browser, you need to [compile the code manually](../README.md#Building from source) and use `embedded-lumina` feature flag.
As a shorthand, `lumina` executable can be renamed to `lumina-node`, which will cause it to act as local node only, same as if invoked with `lumina node`.

## Installation

```bash
cargo install lumina-cli --locked
```

### Running the node

```bash
lumina node --network mocha
```

For all configuration options see `lumina node -h`. By default node will run on mainnet, connecting to official bootstrap nodes, with persistent header store in user's home directory.


