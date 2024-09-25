// Equivalent to `../../node-wasm/js/worker.js` but loads `/wasm/lumina_node_wasm.js`

import init, { NodeWorker } from "/wasm/lumina_node_wasm.js";

Error.stackTraceLimit = 99;

init().then(async () => {
    let worker = new NodeWorker(self);
    console.log("starting worker: ", worker);

    await worker.run();
});
