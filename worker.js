import init, { run_worker, NodeClient, NodeConfig } from "lumina-node-wasm"

export default async function start_worker() {
	await init();
	let channel = new MessageChannel();

	let worker = new Worker(new URL("worker.mjs", import.meta.url));
	worker.post_message(null, [channel.port1()]);


	let client = new NodeClient(channel.port2();
	return client;
}

Error.stackTraceLimit = 99;

let queued = [];
function onMessage(event) {
	queued.push(event);
}

//function onerror(event) { console.error("Worker error:", event); }

if (typeof WorkerGlobalScope !== "undefined" &&
	typeof self !== "undefined" &&
	self instanceof WorkerGlobalScope) {

	init().then(() => {
		run_worker(queued);
	})
}

export { Network, NodeClient, NodeConfig } from "lumina-node-wasm";
