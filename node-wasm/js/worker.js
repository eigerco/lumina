// this file should be installed by wasm-pack in pkg/snippets/<pkg-name>-<hash>/js/
import init, { run_worker } from '../../../lumina_node_wasm.js';

// get the path to this file
export function worker_script_url() {
  return import.meta.url;
}

// if we are in a worker
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  Error.stackTraceLimit = 99;

  // for SharedWorker we queue incoming connections
  // for dedicated Workerwe queue incoming messages (coming from the single client)
  let queued = [];
  if (typeof SharedWorkerGlobalScope !== 'undefined' && self instanceof SharedWorkerGlobalScope) {
    onconnect = (event) => {
      queued.push(event)
    }
  } else {
    onmessage = (event) => {
      queued.push(event);
    }
  }

  await init();
  console.log("starting worker, queued messages: ", queued.length);
  await run_worker(queued);
}
