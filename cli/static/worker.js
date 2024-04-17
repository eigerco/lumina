Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

import init, { run_worker } from "/wasm/lumina_node_wasm.js";

// unfortunately, `init()` takes long enough for the first connection (from the tab which 
// starts Shared Worker) to be missed. As a workaround, we queue connections in js and then
// pass them to Rust, when it's ready. Rust code replaces `onconnect` handler, so this is
// only necessary on startup.
let queued = [];
onconnect = (event) => {
  console.log("Queued connection", event);
  queued.push(event.ports[0]);
}

await init();
await run_worker(queued);

