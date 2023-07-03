use celestia_types::{nmt::Namespace, DataAvailabilityHeader, NamespacedShares};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Share {
    #[method(name = "share.GetSharesByNamespace")]
    async fn share_get_shares_by_namespace(
        &self,
        root: &DataAvailabilityHeader,
        namespace: Namespace,
    ) -> Result<NamespacedShares, Error>;
}
