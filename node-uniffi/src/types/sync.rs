use lumina_node::block_ranges::BlockRange as LuminaBlockRange;
use lumina_node::node::SyncingInfo as LuminaSyncingInfo;
use uniffi::Record;

/// A range of blocks.
#[derive(Record)]
struct BlockRange {
    start: u64,
    end: u64,
}

impl From<LuminaBlockRange> for BlockRange {
    fn from(range: LuminaBlockRange) -> Self {
        Self {
            start: *range.start(),
            end: *range.end(),
        }
    }
}

/// Status of the node syncing.
#[derive(Record)]
pub struct SyncingInfo {
    /// Ranges of headers that are already synchronised
    stored_headers: Vec<BlockRange>,
    /// Syncing target. The latest height seen in the network that was successfully verified.
    subjective_head: u64,
}

impl From<LuminaSyncingInfo> for SyncingInfo {
    fn from(info: LuminaSyncingInfo) -> Self {
        Self {
            stored_headers: info
                .stored_headers
                .into_inner()
                .into_iter()
                .map(BlockRange::from)
                .collect(),
            subjective_head: info.subjective_head,
        }
    }
}
