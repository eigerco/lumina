import { NodeClient } from "lumina-node-wasm"

/**
* Spawn a worker running lumina node and get the `NodeClient` connected to it.
*/
export async function spawnNode() {
  let worker = new Worker(new URL("worker.js", import.meta.url));
  let client = await new NodeClient(worker);
  return client;
}

export * from "lumina-node-wasm";
