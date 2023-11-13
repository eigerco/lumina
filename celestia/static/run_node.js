Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

import init, { Node, default_config } from "/wasm/celestia_node_wasm.js";

async function fetch_config() {
  const response = await fetch('/cfg.json');
  const json = await response.json();

  console.log("Received config:", json);

  let config = default_config(json.network);
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
  const network_div = document.getElementById("network_id");
  const genesis_div = document.getElementById("genesis_hash");
  const bootnodes_div = document.getElementById("bootnodes");

  const update_config_elements = () => {
    network_div.value = window.config.network;
    genesis_div.value = window.config.genesis_hash || "";
    bootnodes_div.value = window.config.bootnodes.join("\n");
  }

  let proxy = {
    set: function(obj, prop, value) {
      if (prop == "network") {
        const config = default_config(Number(value));
        obj.network = config.network;
        obj.genesis_hash = config.genesis_hash;
        obj.bootnodes = config.bootnodes;
      } else if (prop == "genesis_hash" || prop == "bootnodes") {
        obj[prop] = value;
      } else {
        return Reflect.set(obj, prop, value);
      }

      update_config_elements()

      return true;
    }
  };

  window.config = new Proxy(data, proxy);
  update_config_elements();

  network_div.addEventListener("change", (event) => {
    window.config.network = Number(event.target.value);
  });
  genesis_div.addEventListener("change", (event) => {
    window.config.genesis_hash = event.target.value;
  });
  bootnodes_div.addEventListener("change", (event) => {
    window.config.bootnodes = event.target.value.split("\n");
  });
}

async function start_node(config) {
  window.node = await new Node(config);

  document.getElementById("peer_id").innerText = JSON.stringify(await window.node.local_peer_id());
}

async function main(document, window) {
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
