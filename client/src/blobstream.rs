use std::sync::Arc;

use celestia_rpc::{blobstream::BlobstreamClient, Error as CelestiaRpcError};

use crate::client::ClientInner;
use crate::types::hash::Hash;
use crate::types::MerkleProof;
use crate::{Error, Result};

use crate::exec_rpc;

/// Blobstream API for querying bridge nodes.
pub struct BlobstreamApi {
    inner: Arc<ClientInner>,
}

impl BlobstreamApi {
    pub(crate) fn new(inner: Arc<ClientInner>) -> BlobstreamApi {
        BlobstreamApi { inner }
    }

    /// Collects the data roots over a provided ordered range of blocks, and then
    /// creates a new Merkle root of those data roots.
    ///
    /// The range is end exclusive.
    pub async fn get_data_root_tuple_root(&self, start: u64, end: u64) -> Result<Hash> {
        exec_rpc!(self, |rpc| async {
            rpc.blobstream_get_data_root_tuple_root(start, end)
                .await
                .map_err(CelestiaRpcError::from)
        })
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
        exec_rpc!(self, |rpc| async {
            rpc.blobstream_get_data_root_tuple_inclusion_proof(height, start, end)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::ensure_serializable_deserializable;

    #[allow(dead_code)]
    #[allow(unused_variables)]
    #[allow(unreachable_code)]
    #[allow(clippy::diverging_sub_expression)]
    async fn enforce_serde_bounds() {
        // intentionally no run, compile only test
        let api = BlobstreamApi::new(unimplemented!());

        ensure_serializable_deserializable(api.get_data_root_tuple_root(0, 0).await.unwrap());

        ensure_serializable_deserializable(
            api.get_data_root_tuple_inclusion_proof(0, 0, 0)
                .await
                .unwrap(),
        );
    }
}
