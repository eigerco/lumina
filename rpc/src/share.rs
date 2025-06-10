//! celestia-node rpc types and methods related to shares
//!
use std::fmt;
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

/// Side of a row within the EDS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RowSide {
    /// The row data is on the left of the EDS (i.e. in the ODS).
    Left,
    /// The row data is on the right of the EDS (i.e. it's parity data).
    Right,
    /// The row contains both the original data and the parity data.
    Both,
}

/// Response type for [`ShareClient::share_get_row`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetRowResponse {
    /// Shares contained in given range.
    pub shares: Vec<Share>,
    /// Side of the row within the EDS.
    pub side: RowSide,
}

mod rpc {
    use super::*;
    use celestia_types::eds::RawExtendedDataSquare;

    #[rpc(client, namespace = "share", namespace_separator = ".")]
    pub trait Share {
        #[method(name = "GetEDS")]
        async fn share_get_eds(&self, height: u64) -> Result<RawExtendedDataSquare, Error>;

        #[method(name = "GetRange")]
        async fn share_get_range(
            &self,
            height: u64,
            start: u64,
            end: u64,
        ) -> Result<GetRangeResponse, Error>;

        #[method(name = "GetRow")]
        async fn share_get_row(&self, height: u64, row: u64) -> Result<GetRowResponse, Error>;

        #[method(name = "GetShare")]
        async fn share_get_share(&self, height: u64, row: u64, col: u64)
            -> Result<RawShare, Error>;

        #[method(name = "GetNamespaceData")]
        async fn share_get_namespace_data(
            &self,
            height: u64,
            namespace: Namespace,
        ) -> Result<NamespaceData, Error>;

        #[method(name = "SharesAvailable")]
        async fn share_shares_available(&self, height: u64) -> Result<(), Error>;
    }
}

/// Client implementation for the `Share` RPC API.
///
/// Please note that celestia-node requires just the block height for most of those API's.
/// This trait instead requires [`ExtendedHeader`] to perform validation of the returned types.
// NOTE: we use EH wherever Share is returned because it's gonna be required in future
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

            ExtendedDataSquare::from_raw(raw_eds, app_version).map_err(custom_client_error)
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
        async move {
            let resp =
                rpc::ShareClient::share_get_range(self, root.height().value(), start, end).await?;

            let app = root.app_version().map_err(custom_client_error)?;
            for share in &resp.shares {
                share.validate(app).map_err(custom_client_error)?;
            }

            Ok(resp)
        }
    }

    /// GetShare gets the list of shares in a single row.
    fn share_get_row<'a, 'b, 'fut>(
        &'a self,
        root: &'b ExtendedHeader,
        row: u64,
    ) -> impl Future<Output = Result<GetRowResponse, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::ShareClient::share_get_row(self, root.height().value(), row)
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
            let share = if is_ods_square(row, col, root.dah.square_width()) {
                Share::from_raw(&share.data)
            } else {
                Share::parity(&share.data)
            }
            .map_err(custom_client_error)?;

            let app = root.app_version().map_err(custom_client_error)?;
            share.validate(app).map_err(custom_client_error)?;

            Ok(share)
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
        async move {
            let ns_data =
                rpc::ShareClient::share_get_namespace_data(self, root.height().value(), namespace)
                    .await?;

            let app = root.app_version().map_err(custom_client_error)?;
            for shr in ns_data.rows.iter().flat_map(|row| &row.shares) {
                shr.validate(app).map_err(custom_client_error)?;
            }

            Ok(ns_data)
        }
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

fn custom_client_error<E: fmt::Display>(error: E) -> Error {
    Error::Custom(error.to_string())
}
