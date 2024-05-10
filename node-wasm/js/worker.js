// this file should be installed by wasm-pack in pkg/snippets/<pkg-name>-<hash>/js/
import init, { run_worker } from '../../../lumina_node_wasm.js';

// get the path to this file
export function worker_script_url() {
  return import.meta.url;
}

// if we are in a worker
if (
  typeof WorkerGlobalScope !== 'undefined'
  && self instanceof WorkerGlobalScope
) {
  Error.stackTraceLimit = 99;

  let queued = [];
  onconnect = (event) => {
    console.log("Queued connection", event);
    queued.push(event.ports[0]);
  }

  await init();
  await run_worker(queued);
}
