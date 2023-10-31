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


function bind_config(data) {
    let proxy = {
        set: function(obj, prop, value) {
            console.log(obj, prop, value);
            switch (prop) {
                case 'network':
                    document.getElementById("network_id").value = value;
                    break;
                case 'genesis_hash':
                    document.getElementById("genesis_hash").value = value;
                    break;
                case 'bootnodes':
                    document.getElementById("bootnodes").value = value.join("\n");
                    break;
            }
            obj[prop] = value;
            console.log(obj, prop, value);

            return true;
        }
    };
    window.config = new Proxy(data, proxy);
    document.getElementById("network_id").addEventListener("change", (event) => {
        window.config.network = Number(event.target.value);
    });
    document.getElementById("genesis_hash").addEventListener("change", (event) => {
        window.config.genesis_hash = event.target.value;
    });
    document.getElementById("bootnodes").addEventListener("change", (event) => {
        window.config.bootnodes = event.target.value.split("\n");
    });


    document.getElementById("network_id").value = config.network;
    document.getElementById("genesis_hash").value = config.genesis_hash;
    document.getElementById("bootnodes").value = config.bootnodes.join("\n");
}

async function start_node(proxy_config) {
    window.node = await new Node(config);

    document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());
}

async function main(document, window, undefined) {
    await init();

    bind_config(await fetch_config());

    document.getElementById("start").addEventListener("click", async function(ev) {
        document.querySelectorAll('.config').forEach(function(element) {
            element.disabled = true
        });

        start_node(window.config);
    });

    setInterval(async function() { await show_stats(window.node) }, 1000)
}

await main(document, window);

