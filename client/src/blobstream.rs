use std::sync::Arc;

use celestia_rpc::blobstream::BlobstreamClient;

use crate::client::Context;
use crate::types::hash::Hash;
use crate::types::MerkleProof;
use crate::Result;

/// Blobstream API for quering bridge nodes.
pub struct BlobstreamApi {
    ctx: Arc<Context>,
}

impl BlobstreamApi {
    pub(crate) fn new(ctx: Arc<Context>) -> BlobstreamApi {
        BlobstreamApi { ctx }
    }

    /// Collects the data roots over a provided ordered range of blocks, and then
    /// creates a new Merkle root of those data roots.
    ///
    /// The range is end exclusive.
    pub async fn get_data_root_tuple_root(&self, start: u64, end: u64) -> Result<Hash> {
        Ok(self
            .ctx
            .rpc
            .blobstream_get_data_root_tuple_root(start, end)
            .await?)
    }

    /// Creates an inclusion proof, for the data root tuple of block height `height`,
    /// in the set of blocks defined by `start` and `end`.
    ///
    /// The range is end exclusive.
    pub async fn get_data_root_tuple_inclusion_proof(
        &self,
        height: u64,
        start: u64,
        end: u64,
    ) -> Result<MerkleProof> {
        Ok(self
            .ctx
            .rpc
            .blobstream_get_data_root_tuple_inclusion_proof(height, start, end)
            .await?)
    }
}
