import init, { NodeWorkerWrapper, NodeClient } from "lumina-node-wasm"

export default async function start_worker() {
	await init();
	let worker = new Worker(new URL("worker.mjs", import.meta.url));
	let client = new NodeClient(worker);
	return client;
}

Error.stackTraceLimit = 99;

if (typeof WorkerGlobalScope !== "undefined" && typeof self !== "undefined" && self instanceof WorkerGlobalScope) {
	init().then(async () => {
		let worker = new NodeWorkerWrapper();
		console.log("starting worker: ", worker);
		worker.connect(self)

		while (true) {
			await worker.poll();
		}
	});
}

export { Network, NodeClient, NodeConfig } from "lumina-node-wasm";
