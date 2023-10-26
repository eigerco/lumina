Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

import init, { setup_logging, Network, WasmNode, WasmNodeConfig, canonical_network_bootnodes, network_genesis } from "/wasm/wasm_node.js";

async function fetch_config() {
    const response = await fetch('/cfg.json');
    const json = await response.json();

    console.log("Received config:", json);

    const network = json.network;
    const bootnodes = json.bootnodes
    if (bootnodes.length === 0) {
        bootnodes.push(...canonical_network_bootnodes(network));
    }
    const genesis = network_genesis(network);

    //return new WasmNodeConfig(network, genesis, bootnodes);
    return {
        "network": network,
        "genesis": genesis,
        "bootnodes": bootnodes,
    };
}

async function show_stats(node) {
    if (!node) {
        return;
    }
    document.getElementById("syncer").innerText = JSON.stringify(await node.syncer_info());

    let peers_ul = document.createElement('ul');
    (await node.connected_peers()).forEach(function(peer) {
        var li = document.createElement("li");
        li.innerText = peer;
        peers_ul.appendChild(li);
    });

    document.getElementById("peers").replaceChildren(peers_ul);
}


function bind_config() {
    // TODO two way binding between window.config and input values
}

+async function main(document, window, undefined) {
    await init();
    await setup_logging();

    window.config = await fetch_config();

    show_config(window.config);

    document.getElementById("start").addEventListener("click", async function(ev) {
        start_node(window.config);
    });

    await show_stats(window.node);
    setInterval(async function() { await show_stats(window.node) }, 1000)
}(document, window);

function show_config(config) {
    document.getElementById("network_id").value = config.network;
    document.getElementById("genesis").value = config.genesis;
    document.getElementById("bootnodes").value = config.bootnodes.join("\n");
}

async function start_node(config_json) {
    const config = new WasmNodeConfig(config_json.network, config_json.genesis, config_json.bootnodes);

    window.node = await new WasmNode(config);

    document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());
}
