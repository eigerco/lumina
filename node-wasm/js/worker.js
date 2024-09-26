import init, { NodeWorker } from "lumina-node-wasm"

Error.stackTraceLimit = 99;

init().then(async () => {
  let worker = new NodeWorker(self);
  console.log("starting worker: ", worker);

  await worker.run();
});
