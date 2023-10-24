use celestia_types::network::{self, Network};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn setup() {
    console_error_panic_hook::set_once();

    tracing_wasm::set_as_global_default();
}

#[wasm_bindgen]
pub fn canonical_network_bootnodes(network: Network) -> JsValue {
    to_value(&network::canonical_network_bootnodes(network).unwrap_throw()).unwrap_throw()
}

#[wasm_bindgen]
pub fn network_genesis(network: Network) -> JsValue {
    to_value(&network::network_genesis(network).unwrap_throw()).unwrap_throw()
}
