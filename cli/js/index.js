Error.stackTraceLimit = 99; // rust stack traces can get pretty big, increase the default

import { spawnNode, NodeConfig } from "lumina-node";

async function fetch_config() {
  const response = await fetch('/cfg.json');
  const json = await response.json();

  console.log("Received config:", json);

  let config = NodeConfig.default(json.network);
  if (json.bootnodes.length !== 0) {
    config.bootnodes = json.bootnodes;
  }

  return config;
}

async function show_stats(node) {
  if (!node || !await node.is_running()) {
    return;
  }
  const info = await node.syncer_info();
  document.getElementById("stored-ranges").innerText = info.stored_headers.map((range) => {
    return `${range.start}..${range.end}`;
  }).join(", ");

  let peers_ul = document.createElement('ul');
  (await node.connected_peers()).forEach(peer => {
    var li = document.createElement("li");
    li.innerText = peer;
    li.classList.add("mono");
    peers_ul.appendChild(li);
  });

  document.getElementById("peers").replaceChildren(peers_ul);

  const network_head = await node.get_network_head_header();
  if (network_head == null) {
    return
  }

  const square_rows = network_head.dah.row_roots.length;
  const square_cols = network_head.dah.column_roots.length;

  document.getElementById("block-height").innerText = network_head.header.height;
  document.getElementById("block-hash").innerText = network_head.commit.block_id.hash;
  document.getElementById("block-data-square").innerText = `${square_rows}x${square_cols} shares`;
}

function bind_config(data) {
  const network_div = document.getElementById("network-id");
  const bootnodes_div = document.getElementById("bootnodes");

  const update_config_elements = () => {
    network_div.value = window.config.network;
    bootnodes_div.value = window.config.bootnodes.join("\n");
  }

  let proxy = {
    set: function(obj, prop, value) {
      if (prop == "network") {
        const config = NodeConfig.default(Number(value));
        obj.network = config.network;
        obj.bootnodes = config.bootnodes;
      } else if (prop == "bootnodes") {
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

  network_div.addEventListener("change", event => {
    window.config.network = Number(event.target.value.trim());
  });
  bootnodes_div.addEventListener("change", event => {
    window.config.bootnodes = event.target.value.trim().split("\n").map(multiaddr => multiaddr.trim());
  });
}

function log_event(event) {
  // Skip noisy events
  if (event.data.get("event").type == "share_sampling_result") {
    return;
  }

  const time = new Date(event.data.get("time"));

  const log = time.getHours().toString().padStart(2, '0')
    + ":" + time.getMinutes().toString().padStart(2, '0')
    + ":" + time.getSeconds().toString().padStart(2, '0')
    + "." + time.getMilliseconds().toString().padStart(3, '0')
    + ": " + event.data.get("message");

  var textarea = document.getElementById("event-logs");
  textarea.value += log + "\n";
  textarea.scrollTop = textarea.scrollHeight;
}

async function main(document, window) {
  window.node = await spawnNode();

  console.log("AAAAAA");
  function timeout(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }
  await timeout(3000);

  console.log("AAAAAA");
  window.events = await window.node.eventsChannel();
  console.log("AAAAAA");
  window.events.onmessage = (event) => {
    log_event(event);
  };
  console.log("AAAAAA");

  bind_config(await fetch_config());

  if (await window.node.is_running() === true) {
    document.querySelectorAll('.config').forEach(elem => elem.disabled = true);
    document.getElementById("peer-id").innerText = await window.node.local_peer_id();
    document.querySelectorAll(".status").forEach(elem => elem.style.visibility = "visible");
  }

  document.getElementById("start").addEventListener("click", async () => {
    document.querySelectorAll('.config').forEach(elem => elem.disabled = true);

    await window.node.start(window.config);
    document.getElementById("peer-id").innerText = await window.node.local_peer_id();
    document.querySelectorAll(".status").forEach(elem => elem.style.visibility = "visible");
  });

  setInterval(async () => await show_stats(window.node), 1000)
}

await main(document, window);
