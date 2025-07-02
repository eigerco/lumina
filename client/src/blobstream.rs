use std::sync::Arc;

use celestia_grpc::DocSigner;

use crate::Context;

pub struct BlobstreamApi<S> {
    ctx: Arc<Context<S>>,
}

impl<S> BlobstreamApi<S>
where
    S: DocSigner,
{
    pub(crate) fn new(ctx: Arc<Context<S>>) -> BlobstreamApi<S> {
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
}
