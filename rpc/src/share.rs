//! celestia-node rpc types and methods related to shares
//!
use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespacedShares;
use celestia_types::{ExtendedDataSquare, ExtendedHeader, RawShare, Share, ShareProof};
use jsonrpsee::proc_macros::rpc;
use serde::{Deserialize, Serialize};

/// Response type for [`ShareClient::share_get_range`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetRangeResponse {
    /// Shares contained in given range.
    pub shares: Vec<RawShare>,
    /// Proof of inclusion of the shares.
    pub proof: ShareProof,
}

#[rpc(client)]
pub trait Share {
    /// GetEDS gets the full EDS identified by the given root.
    #[method(name = "share.GetEDS")]
    async fn share_get_eds(&self, root: &ExtendedHeader) -> Result<ExtendedDataSquare, Error>;

    /// GetRange gets a list of shares and their corresponding proof.
    #[method(name = "share.GetRange")]
    async fn share_get_range(
        &self,
        height: u64,
        start: usize,
        end: usize,
    ) -> Result<GetRangeResponse, Error>;

    /// GetShare gets a Share by coordinates in EDS.
    #[method(name = "share.GetShare")]
    async fn share_get_share(
        &self,
        root: &ExtendedHeader,
        row: u64,
        col: u64,
    ) -> Result<Share, Error>;

    /// GetSharesByNamespace gets all shares from an EDS within the given namespace. Shares are returned in a row-by-row order if the namespace spans multiple rows.
    #[method(name = "share.GetSharesByNamespace")]
    async fn share_get_shares_by_namespace(
        &self,
        root: &ExtendedHeader,
        namespace: Namespace,
    ) -> Result<NamespacedShares, Error>;

    /// SharesAvailable subjectively validates if Shares committed to the given Root are available on the Network.
    #[method(name = "share.SharesAvailable")]
    async fn share_shares_available(&self, root: &ExtendedHeader) -> Result<(), Error>;
}
