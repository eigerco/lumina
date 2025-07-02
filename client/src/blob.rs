use std::sync::Arc;

use celestia_grpc::DocSigner;

use crate::Context;

pub struct BlobApi<S> {
    ctx: Arc<Context<S>>,
}

impl<S> BlobApi<S>
where
    S: DocSigner,
{
    pub(crate) fn new(ctx: Arc<Context<S>>) -> BlobApi<S> {
        BlobApi { ctx }
    }

    /*
         // Submit sends Blobs and reports the height in which they were included.
    // Allows sending multiple Blobs atomically synchronously.
    // Uses default wallet registered on the Node.
    //
    // WRITE
    Submit(_ context.Context, _ []*blob.Blob, _ *blob.SubmitOptions) (height uint64, _ error)


    // Get retrieves the blob by commitment under the given namespace and height.
    Get(_ context.Context, height uint64, _ libshare.Namespace, _ blob.Commitment) (*blob.Blob, error)
    // GetAll returns all blobs under the given namespaces at the given height.
    // If all blobs were found without any errors, the user will receive a list of blobs.
    // If the BlobService couldn't find any blobs under the requested namespaces,
    // the user will receive an empty list of blobs along with an empty error.
    // If some of the requested namespaces were not found, the user will receive all the found blobs
    // and an empty error. If there were internal errors during some of the requests,
    // the user will receive all found blobs along with a combined error message.
    //
    // All blobs will preserve the order of the namespaces that were requested.
    GetAll(_ context.Context, height uint64, _ []libshare.Namespace) ([]*blob.Blob, error)
    // GetProof retrieves proofs in the given namespaces at the given height by commitment.
    GetProof(_ context.Context, height uint64, _ libshare.Namespace, _ blob.Commitment) (*blob.Proof, error)
    // Included checks whether a blob's given commitment(Merkle subtree root) is included at
    // given height and under the namespace.
    Included(_ context.Context, height uint64, _ libshare.Namespace, _ *blob.Proof, _ blob.Commitment) (bool, error)
    // GetCommitmentProof generates a commitment proof for a share commitment.
    GetCommitmentProof(
        ctx context.Context,
        height uint64,
        namespace libshare.Namespace,
        shareCommitment []byte,
    ) (*blob.CommitmentProof, error)
    // Subscribe to published blobs from the given namespace as they are included.
    Subscribe(_ context.Context, _ libshare.Namespace) (<-chan *blob.SubscriptionResponse, error)
    */
}
