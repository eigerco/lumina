use std::sync::Arc;

use celestia_rpc::{Error as CelestiaRpcError, ShareClient};

use crate::api::share::{GetRangeResponse, GetRowResponse, SampleCoordinates};
use crate::client::ClientInner;
use crate::types::nmt::Namespace;
use crate::types::row_namespace_data::NamespaceData;
use crate::types::sample::Sample;
use crate::types::{ExtendedDataSquare, Share};
use crate::{Error, Result};

use crate::exec_rpc;

/// Share API for quering bridge nodes.
pub struct ShareApi {
    inner: Arc<ClientInner>,
}

impl ShareApi {
    pub(crate) fn new(inner: Arc<ClientInner>) -> ShareApi {
        ShareApi { inner }
    }

    /// Performs a subjective validation to check if the shares committed to the
    /// header at the specified height are available and retrievable from the network.
    ///
    /// Returns `Ok(())` if shares are available.
    pub async fn shares_available(&self, height: u64) -> Result<()> {
        exec_rpc!(self, |rpc| async {
            rpc.share_shares_available(height)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves a specific share from the [`ExtendedDataSquare`] at the given
    /// height  using its row and column coordinates.
    pub async fn get(&self, height: u64, row: u64, column: u64) -> Result<Share> {
        let header = self.inner.get_header_validated(height).await?;
        exec_rpc!(self, |rpc| async {
            rpc.share_get_share(&header, row, column)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves multiple shares from the [`ExtendedDataSquare`] at the given
    /// sample coordinates.
    ///
    /// `coordinates` is a list of `(row, column)`.
    pub async fn get_samples<I, C>(&self, height: u64, coordinates: I) -> Result<Vec<Sample>>
    where
        // `Clone` is required to retry the RPC call in case of network failure.
        I: IntoIterator<Item = C> + Clone,
        C: Into<SampleCoordinates>,
    {
        let header = self.inner.get_header_validated(height).await?;
        exec_rpc!(self, |rpc| async {
            rpc.share_get_samples(&header, coordinates.clone())
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves the complete [`ExtendedDataSquare`] for the specified height.
    pub async fn get_eds(&self, height: u64) -> Result<ExtendedDataSquare> {
        let header = self.inner.get_header_validated(height).await?;
        exec_rpc!(self, |rpc| async {
            rpc.share_get_eds(&header)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves all shares from a specific row of the [`ExtendedDataSquare`]
    /// at the given height.
    pub async fn get_row(&self, height: u64, row: u64) -> Result<GetRowResponse> {
        let header = self.inner.get_header_validated(height).await?;
        exec_rpc!(self, |rpc| async {
            rpc.share_get_row(&header, row)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves all shares that belong to the specified namespace within the
    /// [`ExtendedDataSquare`] at the given height.
    ///
    /// The shares are returned in a row-by-row order, maintaining the original
    /// layout if the namespace spans multiple rows.
    pub async fn get_namespace_data(
        &self,
        height: u64,
        namespace: Namespace,
    ) -> Result<NamespaceData> {
        let header = self.inner.get_header_validated(height).await?;

        exec_rpc!(self, |rpc| async {
            rpc.share_get_namespace_data(&header, namespace)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Retrieves a list of shares and their corresponding proof.
    ///
    /// The start and end index ignores parity shares and corresponds to ODS.
    pub async fn get_range(&self, height: u64, start: u64, end: u64) -> Result<GetRangeResponse> {
        let header = self.inner.get_header_validated(height).await?;
        exec_rpc!(self, |rpc| async {
            rpc.share_get_range(&header, start, end)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::{ensure_serializable, ensure_serializable_deserializable};

    #[allow(dead_code)]
    #[allow(unused_variables)]
    #[allow(unreachable_code)]
    #[allow(clippy::diverging_sub_expression)]
    async fn enforce_serde_bounds() {
        // intentionally no-run, compile only test
        let api = ShareApi::new(unimplemented!());

        let _: () = api.shares_available(0).await.unwrap();

        let coordinates: Vec<SampleCoordinates> =
            ensure_serializable_deserializable(unimplemented!());
        ensure_serializable(api.get_samples(0, coordinates).await.unwrap());

        ensure_serializable(api.get_eds(0).await.unwrap());

        ensure_serializable_deserializable(api.get(0, 0, 0).await.unwrap());

        ensure_serializable_deserializable(api.get_row(0, 0).await.unwrap());

        let namespace = ensure_serializable_deserializable(unimplemented!());
        ensure_serializable_deserializable(api.get_namespace_data(0, namespace).await.unwrap());

        ensure_serializable_deserializable(api.get_range(0, 0, 0).await.unwrap());
    }
}
