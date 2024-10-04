use lumina_node::node::{PeerTrackerInfo, SyncingInfo};
use wasm_bindgen::prelude::*;

/// Statistics of the connected peers
#[wasm_bindgen(inspectable)]
#[derive(Debug)]
pub struct PeerTrackerInfoSnapshot {
    /// Number of the connected peers.
    pub num_connected_peers: u64,
    /// Number of the connected trusted peers.
    pub num_connected_trusted_peers: u64,
}

/// A range of blocks between `start` and `end` height, inclusive
#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone)]
pub struct BlockRange {
    /// First block height in range
    pub start: u64,
    /// Last block height in range
    pub end: u64,
}

/// Status of the synchronization.
#[wasm_bindgen(inspectable)]
#[derive(Debug)]
pub struct SyncingInfoSnapshot {
    /// Ranges of headers that are already synchronised
    #[wasm_bindgen(getter_with_clone)]
    pub stored_headers: Vec<BlockRange>,
    /// Syncing target. The latest height seen in the network that was successfully verified.
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
