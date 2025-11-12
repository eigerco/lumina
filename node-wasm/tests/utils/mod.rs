use libp2p::{Multiaddr, multiaddr::Protocol};

use celestia_rpc::Client;
use celestia_rpc::prelude::*;
use lumina_node_wasm::client::{NodeClient, WasmNodeConfig};
use lumina_node_wasm::utils::Network;
use lumina_node_wasm::worker::NodeWorker;
use rexie::Rexie;
use wasm_bindgen_futures::spawn_local;
use web_sys::MessageChannel;

// uses bridge-0, which has skip-auth enabled
const WS_URL: &str = "ws://127.0.0.1:26658";

pub async fn new_rpc_client() -> Client {
    Client::new(WS_URL).await.unwrap()
}

pub async fn spawn_connected_node(bootnodes: Vec<String>) -> NodeClient {
    let message_channel = MessageChannel::new().unwrap();
    let mut worker = NodeWorker::new(message_channel.port1().into());

    spawn_local(async move {
        worker.run().await.unwrap();
    });

    let client = NodeClient::new(message_channel.port2().into())
        .await
        .unwrap();
    assert!(!client.is_running().await.expect("node ready to be run"));

    client
        .start(&WasmNodeConfig {
            network: Network::Private,
            bootnodes,
            identity_key: None,
            use_persistent_memory: false,
            custom_pruning_window_secs: None,
        })
        .await
        .unwrap();
    assert!(client.is_running().await.expect("running node"));
    client.wait_connected_trusted().await.expect("to connect");

    client
}

pub async fn fetch_bridge_webtransport_multiaddr(client: &Client) -> Multiaddr {
    let bridge_info = client.p2p_info().await.unwrap();

    let mut ma = bridge_info
        .addrs
        .into_iter()
        .find(|ma| {
            let not_localhost = !ma
                .iter()
                .any(|prot| prot == Protocol::Ip4("127.0.0.1".parse().unwrap()));
            let webtransport = ma
                .protocol_stack()
                .any(|protocol| protocol == "webtransport");
            not_localhost && webtransport
        })
        .expect("Bridge doesn't listen on webtransport");

    if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
        ma.push(Protocol::P2p(bridge_info.id.into()))
    }

    ma
}

pub async fn remove_database() -> rexie::Result<()> {
    Rexie::delete("private").await?;
    Rexie::delete("private-blockstore").await?;
    Ok(())
}
