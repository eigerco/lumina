Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

import init, { setup_logging, Network, Node, NodeConfig} from "/wasm/wasm_node.js";

async function fetch_config() {
    const response = await fetch('/cfg.json');
    const json = await response.json();

    console.log("Received config:", json);

    let config = new NodeConfig(json.network);
    if (json.bootnodes.length !== 0) {
        config.bootnodes = json.bootnodes;
    }
    if (json.genesis) {
        config.genesis = json.genesis;
    }

    return config;
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

function show_config(config) {
    document.getElementById("network_id").value = config.network;
    document.getElementById("genesis").value = config.genesis_hash;
    document.getElementById("bootnodes").value = config.bootnodes.join("\n");
}

async function start_node(config) {
    window.node = await new Node(config);

    document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());
}

async function main(document, window, undefined) {
    await init();

    window.config = await fetch_config();

    show_config(window.config);

    document.getElementById("start").addEventListener("click", async function(ev) {
        document.querySelectorAll('.config').forEach(function(element) {
            element.disabled = true
        });

        start_node(window.config);
    });

    setInterval(async function() { await show_stats(window.node) }, 1000)
}

await main(document, window);

