//! celestia-node rpc types and methods related to shares
//!
use std::future::Future;
use std::marker::{Send, Sync};

use celestia_types::consts::appconsts::AppVersion;
use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespaceData;
use celestia_types::{ExtendedDataSquare, ExtendedHeader, RawShare, Share, ShareProof};
use jsonrpsee::core::client::{ClientT, Error};
use jsonrpsee::proc_macros::rpc;
use serde::{Deserialize, Serialize};

/// Response type for [`ShareClient::share_get_range`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetRangeResponse {
    /// Shares contained in given range.
    pub shares: Vec<Share>,
    /// Proof of inclusion of the shares.
    pub proof: ShareProof,
}

mod rpc {
    use super::*;
    use celestia_types::eds::RawExtendedDataSquare;

    #[rpc(client)]
    pub trait Share {
        #[method(name = "share.GetEDS")]
        async fn share_get_eds(&self, height: u64) -> Result<RawExtendedDataSquare, Error>;

        #[method(name = "share.GetRange")]
        async fn share_get_range(
            &self,
            height: u64,
            start: u64,
            end: u64,
        ) -> Result<GetRangeResponse, Error>;

        #[method(name = "share.GetShare")]
        async fn share_get_share(&self, height: u64, row: u64, col: u64)
            -> Result<RawShare, Error>;

        #[method(name = "share.GetNamespaceData")]
        async fn share_get_namespace_data(
            &self,
            height: u64,
            namespace: Namespace,
        ) -> Result<NamespaceData, Error>;

        #[method(name = "share.SharesAvailable")]
        async fn share_shares_available(&self, height: u64) -> Result<(), Error>;
    }
}

/// Client implementation for the `Share` RPC API.
///
/// Please note that celestia-node requires just the block height for most of those API's.
/// This trait instead requires [`ExtendedHeader`] to perform validation of the returned types.
// NOTE: we use EH anyhwhere where Share is returned because it's gonna be required in future,
// to check if shares are allowed to have version 1 in corresponding app version
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

            let raw_eds = rpc::ShareClient::share_get_eds(self, root.height().value()).await?;

            ExtendedDataSquare::from_raw(raw_eds, app_version)
                .map_err(|e| Error::Custom(e.to_string()))
        }
    }

    /// GetRange gets a list of shares and their corresponding proof.
    ///
    /// The start and end index ignores parity shares and corresponds to ODS.
    fn share_get_range<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
        start: u64,
        end: u64,
    ) -> impl Future<Output = Result<GetRangeResponse, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_range(self, root.height().value(), start, end)
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
        async move {
            let share =
                rpc::ShareClient::share_get_share(self, root.height().value(), row, col).await?;
            if is_ods_square(row, col, root.dah.square_width()) {
                Share::from_raw(&share.data)
            } else {
                Share::parity(&share.data)
            }
            .map_err(|e| Error::Custom(e.to_string()))
        }
    }

    /// GetNamespaceData gets all shares from an EDS within the given namespace.
    ///
    /// Shares are returned in a row-by-row order if the namespace spans multiple rows.
    ///
    /// PARITY and TAIL PADDING namespaces are not allowed.
    fn share_get_namespace_data<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
        namespace: Namespace,
    ) -> impl Future<Output = Result<NamespaceData, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_namespace_data(self, root.height().value(), namespace)
    }

    /// SharesAvailable subjectively validates if Shares committed to the given Root are available on the Network.
    fn share_shares_available<'a, 'fut>(
        &'a self,
        height: u64,
    ) -> impl Future<Output = Result<(), Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_shares_available(self, height)
    }
}

impl<T> ShareClient for T where T: ClientT {}

fn is_ods_square(row: u64, column: u64, square_width: u16) -> bool {
    let ods_width = square_width / 2;
    row < ods_width as u64 && column < ods_width as u64
}
