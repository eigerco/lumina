use celestia_types::nmt::Namespace;
use celestia_types::{DataAvailabilityHeader, ExtendedDataSquare, NamespacedShares, Share};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Share {
    /// GetEDS gets the full EDS identified by the given root.
    #[method(name = "share.GetEDS")]
    async fn share_get_eds(
        &self,
        root: &DataAvailabilityHeader,
    ) -> Result<ExtendedDataSquare, Error>;

    /// GetShare gets a Share by coordinates in EDS.
    #[method(name = "share.GetShare")]
    async fn share_get_share(
        &self,
        root: &DataAvailabilityHeader,
        row: u64,
        col: u64,
    ) -> Result<Share, Error>;

    /// GetSharesByNamespace gets all shares from an EDS within the given namespace. Shares are returned in a row-by-row order if the namespace spans multiple rows.
    #[method(name = "share.GetSharesByNamespace")]
    async fn share_get_shares_by_namespace(
        &self,
        root: &DataAvailabilityHeader,
        namespace: Namespace,
    ) -> Result<NamespacedShares, Error>;

    /// ProbabilityOfAvailability calculates the probability of the data square being available based on the number of samples collected.
    #[method(name = "share.ProbabilityOfAvailability")]
    async fn share_probability_of_availability(&self) -> Result<f64, Error>;

    /// SharesAvailable subjectively validates if Shares committed to the given Root are available on the Network.
    #[method(name = "share.SharesAvailable")]
    async fn share_shares_available(&self, root: &DataAvailabilityHeader) -> Result<(), Error>;
}
