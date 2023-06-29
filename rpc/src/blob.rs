use celestia_proto::share::p2p::shrex::nd::Proof as RawProof;
use celestia_types::nmt::Namespace;
use celestia_types::Blob;
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Blob {
    #[method(name = "blob.Get")]
    async fn blob_get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: &[u8],
    ) -> Result<Blob, Error>;

    #[method(name = "blob.GetProof")]
    async fn blob_get_proof(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: &[u8],
    ) -> Result<Vec<RawProof>, Error>;

    #[method(name = "blob.Submit")]
    async fn blob_submit(&self, blobs: &[Blob]) -> Result<u64, Error>;
}
