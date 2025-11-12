#![cfg(target_arch = "wasm32")]

use std::time::Duration;

use celestia_rpc::prelude::*;
use celestia_types::p2p::PeerId;
use gloo_timers::future::sleep;
use lumina_node_wasm::utils::setup_logging;
use wasm_bindgen_test::wasm_bindgen_test;

mod utils;

use crate::utils::{
    fetch_bridge_webtransport_multiaddr, new_rpc_client, remove_database, spawn_connected_node,
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// NOTE: This test is split from the rest because when it runs in the same
// browser session with others it may randomly fail. We couldn't identify the
// issue yet, but we believe it's in the WebTransport layer of Chromium or libp2p.
//
// The side effect of the issue is that WebTransport connection to newly discovered
// peers can not be established.
#[wasm_bindgen_test]
async fn discover_network_peers() {
    setup_logging();
    remove_database().await.expect("failed to clear db");
    let rpc_client = new_rpc_client().await;
    let bridge_ma = fetch_bridge_webtransport_multiaddr(&rpc_client).await;

    // wait for other nodes to connect to bridge
    while rpc_client.p2p_peers().await.unwrap().is_empty() {
        sleep(Duration::from_millis(200)).await;
    }

    let client = spawn_connected_node(vec![bridge_ma.to_string()]).await;

    let info = client.network_info().await.unwrap();
    assert_eq!(info.num_peers, 1);

    sleep(Duration::from_secs(1)).await;

    let info = client.network_info().await.unwrap();
    assert!(info.num_peers > 1);

    rpc_client
        .p2p_close_peer(&PeerId(
            client.local_peer_id().await.unwrap().parse().unwrap(),
        ))
        .await
        .unwrap();
}
