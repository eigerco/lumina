Error.stackTraceLimit = 99;

import init, { setup, Network, WasmNode, WasmNodeConfig, canonical_network_bootnodes, network_genesis } from "/wasm/wasm_node.js";

// initialize wasm
await init();
// setup logging and console panic hook
await setup();

const response = await fetch('/cfg.json');
const json = await response.json();

console.log("Received config:", json);

const network = json.network;
let bootnodes = json.bootnodes
if (bootnodes.length === 0) {
    bootnodes = canonical_network_bootnodes(network);
}
const genesis = network_genesis(network);

document.getElementById("network_id").value = network;
document.getElementById("genesis").value = genesis;
document.getElementById("bootnodes").value = bootnodes.join("\n");

document.getElementById("start").addEventListener("click", async function(ev) {
    document.getElementById("start").setAttribute("disabled", "disabled");

    const network = Number(document.getElementById("network_id").value);
    const genesis = document.getElementById("genesis").value;
    const bootnodes  = document.getElementById("bootnodes").value.split("\n");

    console.log("starting with:", network, bootnodes, genesis);

    const config = new WasmNodeConfig(network, genesis, bootnodes);
    window.node = await new WasmNode(config);

    setInterval(async function() {
        document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());
        document.getElementById("syncer").innerText = JSON.stringify(await window.node.syncer_info());
        document.getElementById("peers").innerText = JSON.stringify(await window.node.connected_peers());
    }, 1000);

}, false);
