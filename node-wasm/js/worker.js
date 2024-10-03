import { NodeWorker } from "lumina-node-wasm"

Error.stackTraceLimit = 99;

let worker = new NodeWorker(self);
console.log("Starting NodeWorker");

worker.run();
