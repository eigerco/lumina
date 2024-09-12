import init, { NodeClient } from "lumina-node-wasm"

export async function spawnNode() {
    await init();
    let worker = new Worker(new URL("worker.js", import.meta.url));
    let client = await new NodeClient(worker);
    return client;
}

export * from "lumina-node-wasm";
export default init;
