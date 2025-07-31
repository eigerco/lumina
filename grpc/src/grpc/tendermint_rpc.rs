use celestia_proto::tendermint_celestia_mods::rpc::grpc::{
    StatusRequest, StatusResponse as RawStatusResponse,
};
use celestia_types::{Hash, Height};
use tendermint::{validator, AppHash, Time};
use tendermint_proto::v0_38::p2p::DefaultNodeInfo;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::prelude::wasm_bindgen;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::JsValue;

use crate::grpc::{make_empty_params, make_response_identity};

/// A state of the blockchain synchronization of consensus node.
#[derive(Clone, Debug, PartialEq, Eq)]
// #[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
    wasm_bindgen(getter_with_clone)
)]
pub struct SyncInfo {
    /// Hash of the latest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(js_name = latestBlockHash)
    )]
    pub latest_block_hash: Hash,

    /// App hash of the latest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(js_name = latestAppHash)
    )]
    pub latest_app_hash: AppHash,

    /// Height of the latest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub latest_block_height: Height,

    /// Time of the latest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub latest_block_time: Time,

    /// Hash of the earliest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(js_name = earliestBlockHash)
    )]
    pub earliest_block_hash: Hash,

    /// App hash of the earliest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(js_name = earliestAppHash)
    )]
    pub earliest_app_hash: AppHash,

    /// Height of the earliest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub earliest_block_height: Height,

    /// Time of the earliest block synced by the node
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub earliest_block_time: Time,

    /// Indicates if node is still during initial sync
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(js_name = catchingUp)
    )]
    pub catching_up: bool,
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
#[wasm_bindgen]
impl SyncInfo {
    /// Height of the latest block synced by the node
    #[wasm_bindgen(getter, js_name = latestBlockHeight)]
    pub fn js_latest_block_height(&self) -> u64 {
        self.latest_block_height.value()
    }

    /// Time of the latest block synced by the node
    #[wasm_bindgen(getter, js_name = latestBlockTime)]
    pub fn js_latest_block_time(&self) -> Result<f64, JsValue> {
        Ok(self
            .latest_block_time
            .duration_since(Time::unix_epoch())
            .map_err(|e| JsError::new(&e.to_string()))?
            .as_secs_f64()
            * 1000.0)
    }

    /// Height of the earliest block synced by the node
    #[wasm_bindgen(getter, js_name = earliestBlockHeight)]
    pub fn js_earliest_block_height(&self) -> u64 {
        self.earliest_block_height.value()
    }

    /// Time of the earliest block synced by the node
    #[wasm_bindgen(getter, js_name = earliestBlockTime)]
    pub fn js_earliest_block_time(&self) -> Result<f64, JsValue> {
        Ok(self
            .earliest_block_time
            .duration_since(Time::unix_epoch())
            .map_err(|e| JsError::new(&e.to_string()))?
            .as_secs_f64()
            * 1000.0)
    }
}

/// A state of the blockchain synchronization of consensus node.
#[derive(Clone, Debug, PartialEq)]
// #[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
    wasm_bindgen(getter_with_clone)
)]
pub struct StatusResponse {
    node_info: DefaultNodeInfo,
    sync_info: SyncInfo,
    validator_info: validator::Info,
}

make_response_identity!(StatusResponse);
make_empty_params!(StatusRequest);
