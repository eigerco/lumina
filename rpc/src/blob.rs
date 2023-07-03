use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::{Blob, Commitment};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Blob {
    #[method(name = "blob.Get")]
    async fn blob_get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Blob, Error>;

    #[method(name = "blob.GetProof")]
    async fn blob_get_proof(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Vec<NamespaceProof>, Error>;

    #[method(name = "blob.Submit")]
    async fn blob_submit(&self, blobs: &[Blob]) -> Result<u64, Error>;
}
