import init, { NodeWorker } from "lumina-node-wasm"

Error.stackTraceLimit = 99;

init().then(async () => {
  let worker = new NodeWorker(self);
  console.log("Starting NodeWorker");

  await worker.run();
});
