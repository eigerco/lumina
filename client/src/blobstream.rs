use std::sync::Arc;

use celestia_rpc::blobstream::BlobstreamClient;

use crate::{Context, Result};

const DATA_ROOT_TUPLE_ROOT_BLOCKS_LIMIT: u64 = 10_000;

pub struct BlobstreamApi {
    ctx: Arc<Context>,
}

impl BlobstreamApi {
    pub(crate) fn new(ctx: Arc<Context>) -> BlobstreamApi {
        BlobstreamApi { ctx }
    }

    /*
    // GetDataRootTupleRoot collects the data roots over a provided ordered range of blocks,
    // and then creates a new Merkle root of those data roots. The range is end exclusive.
    // It's in the header module because it only needs access to the headers to generate the proof.
    GetDataRootTupleRoot(ctx context.Context, start, end uint64) (*DataRootTupleRoot, error)

    // GetDataRootTupleInclusionProof creates an inclusion proof, for the data root tuple of block
    // height `height`, in the set of blocks defined by `start` and `end`. The range
    // is end exclusive.
    // It's in the header module because it only needs access to the headers to generate the proof.
    GetDataRootTupleInclusionProof(
        ctx context.Context,
        height, start, end uint64,
    ) (*DataRootTupleInclusionProof, error)
     */

    /// Collects the data roots over a provided ordered range of blocks, and then
    /// creates a new Merkle root of those data roots. The range is end exclusive.

    // It's in the header module because it only needs access to the headers to generate the proof.

    /*
        pub async fn get_data_root_ruple_root(&self, start: u64, end: u64) -> Result<Bytes> {
            /*
            let hex_root = self
                .ctx
                .rpc
                .blobstream_get_data_root_tuple_root(start, end)
                .await?;
            */
            todo!();
        }
    */

    pub async fn get_data_root_tuple_inclusion_proof(
        &self,
        height: u64,
        start: u64,
        end: u64,
    ) -> Result<()> {
        todo!();
    }
}
