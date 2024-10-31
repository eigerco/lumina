//! celestia-node rpc types and methods related to shares
//!
use std::future::Future;
use std::marker::{Send, Sync};

use celestia_types::consts::appconsts::AppVersion;
use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespacedShares;
use celestia_types::{ExtendedDataSquare, ExtendedHeader, RawShare, Share, ShareProof};
use futures::FutureExt;
use jsonrpsee::core::client::{ClientT, Error};
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

mod rpc {
    use super::*;
    use celestia_types::eds::RawExtendedDataSquare;

    #[rpc(client)]
    pub trait Share {
        #[method(name = "share.GetEDS")]
        async fn share_get_eds(
            &self,
            root: &ExtendedHeader,
        ) -> Result<RawExtendedDataSquare, Error>;

        #[method(name = "share.GetRange")]
        async fn share_get_range(
            &self,
            height: u64,
            start: usize,
            end: usize,
        ) -> Result<GetRangeResponse, Error>;

        #[method(name = "share.GetShare")]
        async fn share_get_share(
            &self,
            root: &ExtendedHeader,
            row: u64,
            col: u64,
        ) -> Result<RawShare, Error>;

        #[method(name = "share.GetSharesByNamespace")]
        async fn share_get_shares_by_namespace(
            &self,
            root: &ExtendedHeader,
            namespace: Namespace,
        ) -> Result<NamespacedShares, Error>;

        #[method(name = "share.SharesAvailable")]
        async fn share_shares_available(&self, root: &ExtendedHeader) -> Result<(), Error>;
    }
}

/// Client implementation for the `Share` RPC API.
pub trait ShareClient: ClientT {
    /// GetEDS gets the full EDS identified by the given root.
    fn share_get_eds<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
    ) -> impl Future<Output = Result<ExtendedDataSquare, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        async move {
            let app_version = root.header.version.app;
            let app_version = AppVersion::from_u64(app_version).ok_or_else(|| {
                let e = format!("Invalid or unsupported AppVersion: {app_version}");
                Error::Custom(e)
            })?;

            let raw_eds = rpc::ShareClient::share_get_eds(self, root).await?;

            ExtendedDataSquare::from_raw(raw_eds, app_version)
                .map_err(|e| Error::Custom(e.to_string()))
        }
    }

    /// GetRange gets a list of shares and their corresponding proof.
    fn share_get_range<'a, 'b, 'fut>(
        &'a self,
        height: u64,
        start: usize,
        end: usize,
    ) -> impl Future<Output = Result<GetRangeResponse, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_range(self, height, start, end)
    }

    /// GetShare gets a Share by coordinates in EDS.
    fn share_get_share<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
        row: u64,
        col: u64,
    ) -> impl Future<Output = Result<Share, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_share(self, root, row, col).map(move |res| {
            res.and_then(|shr| {
                if row < root.dah.square_width() as u64 / 2
                    && col < root.dah.square_width() as u64 / 2
                {
                    Share::from_raw(&shr.data)
                } else {
                    Share::parity(&shr.data)
                }
                .map_err(|e| Error::Custom(e.to_string()))
            })
        })
    }

    /// GetSharesByNamespace gets all shares from an EDS within the given namespace. Shares are returned in a row-by-row order if the namespace spans multiple rows.
    fn share_get_shares_by_namespace<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
        namespace: Namespace,
    ) -> impl Future<Output = Result<NamespacedShares, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_shares_by_namespace(self, root, namespace)
    }

    /// SharesAvailable subjectively validates if Shares committed to the given Root are available on the Network.
    fn share_shares_available<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
    ) -> impl Future<Output = Result<(), Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_shares_available(self, root)
    }
}

impl<T> ShareClient for T where T: ClientT {}
