//! celestia-node rpc types and methods related to blobstream

use std::future::Future;
use std::marker::{Send, Sync};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use celestia_types::consts::HASH_SIZE;
use celestia_types::hash::Hash;
use celestia_types::MerkleProof;
use jsonrpsee::core::client::{ClientT, Error};
use jsonrpsee::proc_macros::rpc;

mod rpc {
    use super::*;

    #[rpc(client, namespace = "blobstream", namespace_separator = ".")]
    pub trait Blobstream {
        /// Collects the data roots over a provided ordered range of blocks, and then
        /// creates a new Merkle root of those data roots.
        ///
        /// The range is end exclusive.
        #[method(name = "GetDataRootTupleRoot")]
        async fn blobstream_get_data_root_tuple_root(
            &self,
            start: u64,
            end: u64,
        ) -> Result<String, Error>;

        /// Creates an inclusion proof, for the data root tuple of block height `height`,
        /// in the set of blocks defined by `start` and `end`.
        ///
        /// The range is end exclusive.
        #[method(name = "GetDataRootTupleInclusionProof")]
        async fn blobstream_get_data_root_tuple_inclusion_proof(
            &self,
            height: u64,
            start: u64,
            end: u64,
        ) -> Result<MerkleProof, Error>;
    }
}

/// Client implementation for the Blobstream RPC API.
// TODO: get rid of this after a release or two
// https://github.com/eigerco/lumina/issues/683
pub trait BlobstreamClient: ClientT {
    /// Collects the data roots over a provided ordered range of blocks, and then
    /// creates a new Merkle root of those data roots.
    ///
    /// The range is end exclusive.
    fn blobstream_get_data_root_tuple_root<'a, 'fut>(
        &'a self,
        start: u64,
        end: u64,
    ) -> impl Future<Output = Result<Hash, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        async move {
            let root = rpc::BlobstreamClient::blobstream_get_data_root_tuple_root(self, start, end)
                .await?;

            if root.len() == 2 * HASH_SIZE {
                root.parse::<Hash>()
                    .map_err(|e| Error::Custom(e.to_string()))
            } else {
                let decoded = BASE64
                    .decode(&root)
                    .map_err(|e| Error::Custom(e.to_string()))?;
                Hash::try_from(decoded).map_err(|e| Error::Custom(e.to_string()))
            }
        }
    }

    /// Creates an inclusion proof, for the data root tuple of block height `height`,
    /// in the set of blocks defined by `start` and `end`.
    ///
    /// The range is end exclusive.
    fn blobstream_get_data_root_tuple_inclusion_proof<'a, 'fut>(
        &'a self,
        height: u64,
        start: u64,
        end: u64,
    ) -> impl Future<Output = Result<MerkleProof, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobstreamClient::blobstream_get_data_root_tuple_inclusion_proof(
            self, height, start, end,
        )
    }
}

impl<T> BlobstreamClient for T where T: ClientT {}
