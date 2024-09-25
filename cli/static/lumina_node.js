// Equivalent to `../../node-wasm/js/index.js` but loads `/wasm/lumina_node_wasm.js`

import init, { NodeClient } from "/wasm/lumina_node_wasm.js"

/**
* Spawn a worker running lumina node and get the `NodeClient` connected to it.
*/
export async function spawnNode() {
    await init();
    let worker = new Worker(new URL("/js/worker.js", import.meta.url), { type: 'module' });
    let client = await new NodeClient(worker);

    // Workaround
    await (new Promise(resolve => setTimeout(resolve, 500)));

    return client;
}

export * from "/wasm/lumina_node_wasm.js";
export default init;
