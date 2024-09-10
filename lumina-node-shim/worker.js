Error.stackTraceLimit = 99;

import init, { NodeWorker, NodeClient } from "lumina-node-wasm"

export default async function start_worker() {
    await init();
    let worker = new Worker(new URL("worker.js", import.meta.url));
    let client = new NodeClient(worker);
    return client;
}

if (typeof WorkerGlobalScope !== "undefined" && typeof self !== "undefined" && self instanceof WorkerGlobalScope) {
    init().then(async () => {
        let worker = new NodeWorker();
        console.log("starting worker: ", worker);
        worker.connect(self)

        while (true) {
            await worker.poll();
        }
    });
}

export * from "lumina-node-wasm";
