Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

// The worker has its own scope and no direct access to functions/objects of the
// global scope. We import the generated JS file to make `wasm_bindgen`
// available which we need to initialize our Wasm code.
//importScripts('/wasm/lumina_node_wasm.js');
import init, { run_worker } from "/wasm/lumina_node_wasm.js";

let queued = [];

onconnect = (event) => {
  console.log("Queued connection", event);
  queued.push(event.ports[0]);
}

console.log('Initializing worker')

await init();

console.log('init run')

await run_worker(queued);


/*
// In the worker, we have a different struct that we want to use as in
// `index.js`.
//const {NumberEval} = wasm_bindgen;

async function fetch_config() {
  const response = await fetch('/cfg.json');
  const json = await response.json();

  console.log("Received config:", json);

  let config = NodeConfig.default(json.network);
  if (json.bootnodes.length !== 0) {
    config.bootnodes = json.bootnodes;
  }
  if (json.genesis) {
    config.genesis = json.genesis;
  }

  return config;
}

async function init_wasm_in_worker() {
    await init();
    // Load the wasm file by awaiting the Promise returned by `wasm_bindgen`.
    //await wasm_bindgen('./pkg/wasm_in_web_worker_bg.wasm');

    // Create a new object of the `NumberEval` struct.
    //var num_eval = NumberEval.new();

    let config = await fetch_config();

    let node = await new Node(config);

    // Set callback to handle messages passed to the worker.
    self.onmessage = async event => {
        console.log("worker", node);
        console.log("ev in worker", event);
        // By using methods of a struct as reaction to messages passed to the
        // worker, we can preserve our state between messages.
        //var worker_result = num_eval.is_even(event.data);

        // Send response back to be handled by callback in main thread.
        //self.postMessage(worker_result);
    };
};

init_wasm_in_worker();
*/
