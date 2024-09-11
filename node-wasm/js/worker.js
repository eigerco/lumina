import init, { NodeWorker, NodeClient } from "lumina-node-wasm"

Error.stackTraceLimit = 99;

init().then(async () => {
    let worker = new NodeWorker();
    console.log("starting worker: ", worker);
    worker.connect(self)

    while (true) {
        await worker.poll();
    }
});
