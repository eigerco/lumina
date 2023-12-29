# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

```javascript
import init, { Node, NodeConfig } from "/wasm/lumina_node_wasm.js";

await init();

const config = NodeConfig.default(0); // 0 for mainnet
const node = await new Node(config);

await node.wait_connected();
await node.request_head_header();
```
