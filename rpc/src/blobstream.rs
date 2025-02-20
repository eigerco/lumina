use bytes::Bytes;
use celestia_types::MerkleProof;
use jsonrpsee::proc_macros::rpc;
use prost::bytes;

// DataRootTupleRoot is the root of the merkle tree created
// from a set of data root tuples.
pub type DataRootTupleRoot = Bytes;

// DataRootTupleInclusionProof is the binary merkle
// inclusion proof of a height to a data commitment.
pub type DataRootTupleInclusionProof = MerkleProof;

#[rpc(client)]
pub trait Blobstream {
    /// GetDataRootTupleRoot retrieves the data root tuple root for a given range from start to end
    #[method(name = "blobstream.GetDataRootTupleRoot")]
    async fn get_data_root_tuple_root(
        &self,
        start: u64,
        end: u64,
    ) -> Result<DataRootTupleRoot, Error>;

    /// GetDataRootTupleInclusionProof returns a data root tuple inclusion proof for a given height
    /// between a range from start to end
    #[method(name = "blobstream.GetDataRootTupleInclusionProof")]
    async fn get_data_root_tuple_inclusion_proof(
        &self,
        height: u64,
        start: u64,
        end: u64,
    ) -> Result<DataRootTupleInclusionProof, Error>;
}
