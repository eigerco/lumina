# Lumina node wasm

A compatibility layer for the [`Lumina`](https://github.com/eigerco/lumina) node to
work within a browser environment and be operable with javascript.

# Example
Starting lumina inside a dedicated worker

```javascript
import { spawnNode, NodeConfig, Network } from "lumina-node";

const node = await spawnNode();
const mainnetConfig = NodeConfig.default(Network.Mainnet);

await node.start(mainnetConfig);

await node.wait_connected();
await node.request_head_header();
```

## Manual setup

Note that `spawnNode` implicitly calls wasm initialisation code. If you want to set things up manually, make sure to call the default export before using any of the wasm functionality.

```javascript
import init, { NodeConfig, Network } from "lumina-node";

await init();
const config = NodeConfig.default(Network.Mainnet);
const worker = new NodeWorker();

const channel = new MessageChannel();
worker.connect(channel.port1);
const client = await new NodeClient(channel.port2);

// note that this runs lumina in the current context (and not in a worker, like `spawnNode`). runWorker doesn't return.
worker.runWorker();

await client.wait_connected();
await client.request_head_header();
```
