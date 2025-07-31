//! celestia-node rpc types and methods related to blobstream
use celestia_types::MerkleProof;
use jsonrpsee::proc_macros::rpc;
use prost::bytes::Bytes;

#[rpc(client, namespace = "blobstream", namespace_separator = ".")]
pub trait Blobstream {
    /// Collects the data roots over a provided ordered range of blocks, and then
    /// creates a new Merkle root of those data roots.
    ///
    /// The range is end exclusive.
    #[method(name = "GetDataRootTupleRoot")]
    // TODO: This should return `Hash` when celestia-node#4390 is released to mainnet.
    async fn blobstream_get_data_root_tuple_root(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Bytes, Error>;

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
