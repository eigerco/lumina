//! celestia-node rpc types and methods related to blobstream

use celestia_types::{hash::Hash, MerkleProof};
use jsonrpsee::proc_macros::rpc;

/// DataRootTupleInclusionProof is the binary merkle
/// inclusion proof of a height to a data commitment.
pub type DataRootTupleInclusionProof = MerkleProof;

#[rpc(client)]
pub trait Blobstream {
    /// GetDataRootTupleRoot retrieves the data root tuple root for a given range from start to end
    #[method(name = "blobstream.GetDataRootTupleRoot")]
    async fn blobstream_get_data_root_tuple_root(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Hash, Error>;

    /// GetDataRootTupleInclusionProof returns a data root tuple inclusion proof for a given height
    /// between a range from start to end
    #[method(name = "blobstream.GetDataRootTupleInclusionProof")]
    async fn blobstream_get_data_root_tuple_inclusion_proof(
        &self,
        height: u64,
        start: u64,
        end: u64,
    ) -> Result<DataRootTupleInclusionProof, Error>;
}
