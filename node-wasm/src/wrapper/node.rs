use lumina_node::node::{PeerTrackerInfo, SyncingInfo};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inspectable)]
#[derive(Debug)]
pub struct PeerTrackerInfoSnapshot {
    #[wasm_bindgen(js_name = "numConnectedPeers")]
    pub num_connected_peers: u64,
    #[wasm_bindgen(js_name = "numConnectedTrustedPeers")]
    pub num_connected_trusted_peers: u64,
}

#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone)]
pub struct BlockRange {
    pub start: u64,
    pub end: u64,
}

#[wasm_bindgen(inspectable)]
#[derive(Debug)]
pub struct SyncingInfoSnapshot {
    #[wasm_bindgen(getter_with_clone, js_name = "storedHeaders")]
    pub stored_headers: Vec<BlockRange>,
    #[wasm_bindgen(js_name = "subjectiveHead")]
    pub subjective_head: u64,
}

impl From<PeerTrackerInfo> for PeerTrackerInfoSnapshot {
    fn from(value: PeerTrackerInfo) -> Self {
        Self {
            num_connected_peers: value.num_connected_peers,
            num_connected_trusted_peers: value.num_connected_trusted_peers,
        }
    }
}

impl From<SyncingInfo> for SyncingInfoSnapshot {
    fn from(value: SyncingInfo) -> Self {
        let stored_headers = value
            .stored_headers
            .into_inner()
            .iter()
            .map(|r| BlockRange {
                start: *r.start(),
                end: *r.end(),
            })
            .collect();
        Self {
            stored_headers,
            subjective_head: value.subjective_head,
        }
    }
}
