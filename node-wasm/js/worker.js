import { NodeWorker, NodeClient } from "lumina-node-wasm"

Error.stackTraceLimit = 99;

let worker = new NodeWorker(self);
console.log("starting worker: ", worker);

worker.run();
