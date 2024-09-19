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

// client and worker accept any object with MessagePort like interface e.g. Worker
const channel = new MessageChannel();
const worker = new NodeWorker(channel.port1);

// note that this runs lumina in the current context (and doesn't create a new web-worker). runWorker doesn't return.
const worker_promise = worker.run();

// client port can be used locally or transferred like any plain MessagePort
const client = await new NodeClient(channel.port2);
await client.wait_connected();
await client.request_head_header();
```
