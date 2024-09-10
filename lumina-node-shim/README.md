# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

# Example
Starting lumina inside a dedicated worker

```javascript
import init, { Node, NodeConfig, Network } from "lumina-node";

const node = await init();
const config = NodeConfig.default(Network.Mainnet);

await node.start(config);

await node.wait_connected();
await node.request_head_header();
```
