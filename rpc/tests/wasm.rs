#![cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]

use celestia_rpc::client::Client;
use celestia_rpc::prelude::*;
use wasm_bindgen_test::*;

// uses bridge-1, which has skip-auth enabled
const CELESTIA_RPC_URL: &str = "ws://localhost:36658";

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn network_head() {
    let client = Client::new(CELESTIA_RPC_URL).await.unwrap();
    let network_head = client.header_network_head().await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let adjacent_header = client
        .header_get_by_height(network_head.height().value() - 1)
        .await
        .unwrap();

    network_head.validate().unwrap();
    genesis_header.verify(&network_head).unwrap();
    adjacent_header.verify(&network_head).unwrap();
}
