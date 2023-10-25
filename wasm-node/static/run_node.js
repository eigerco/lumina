Error.stackTraceLimit = 99;

import init, { setup_logging, Network, WasmNode, WasmNodeConfig, canonical_network_bootnodes, network_genesis } from "/wasm/wasm_node.js";

// initialize wasm
await init();
// setup logging and console panic hook
await setup_logging();

const response = await fetch('/cfg.json');
const json = await response.json();

console.log("Received config:", json);

const network = json.network;
const bootnodes = json.bootnodes
if (bootnodes.length === 0) {
    bootnodes.push(...canonical_network_bootnodes(network));
}
const genesis = network_genesis(network);

document.getElementById("network_id").value = network;
document.getElementById("genesis").value = genesis;
document.getElementById("bootnodes").value = bootnodes.join("\n");

document.getElementById("start").addEventListener("click", async function(ev) {
    document.getElementById("start").disabled = true;

    const network = Number(document.getElementById("network_id").value);
    const genesis = document.getElementById("genesis").value;
    const bootnodes  = document.getElementById("bootnodes").value.split("\n");

    console.log("starting with:", network, bootnodes, genesis);

    const config = new WasmNodeConfig(network, genesis, bootnodes);
    window.node = await new WasmNode(config);

    document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());

    async function update_stats() {
        document.getElementById("syncer").innerText = JSON.stringify(await window.node.syncer_info());
        document.getElementById("peers").innerText = JSON.stringify(await window.node.connected_peers());

        setTimeout(update_stats, 1000)
    }
    await update_stats();

}, false);
