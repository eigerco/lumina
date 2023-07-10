use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::{Blob, Commitment};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Blob {
    /// Get retrieves the blob by commitment under the given namespace and height.
    #[method(name = "blob.Get")]
    async fn blob_get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Blob, Error>;

    /// GetAll returns all blobs under the given namespaces and height.
    #[method(name = "blob.GetAll")]
    async fn blob_get_all(&self, height: u64, namespaces: &[Namespace])
        -> Result<Vec<Blob>, Error>;

    /// GetProof retrieves proofs in the given namespaces at the given height by commitment.
    #[method(name = "blob.GetProof")]
    async fn blob_get_proof(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Vec<NamespaceProof>, Error>;

    /// Included checks whether a blob's given commitment(Merkle subtree root) is included at given height and under the namespace.
    #[method(name = "blob.Included")]
    async fn blob_included(
        &self,
        height: u64,
        namespace: Namespace,
        proof: &NamespaceProof,
        commitment: Commitment,
    ) -> Result<bool, Error>;

    /// Submit sends Blobs and reports the height in which they were included. Allows sending multiple Blobs atomically synchronously. Uses default wallet registered on the Node.
    #[method(name = "blob.Submit")]
    async fn blob_submit(&self, blobs: &[Blob]) -> Result<u64, Error>;
}
