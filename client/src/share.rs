use std::sync::Arc;

use celestia_rpc::{HeaderClient, ShareClient};
use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespaceData;
use celestia_types::sample::Sample;
use celestia_types::{ExtendedDataSquare, Height, Share};

use crate::client::Context;
use crate::Result;

pub use celestia_rpc::share::{GetRangeResponse, GetRowResponse, RowSide, SampleCoordinates};

/// Share API for quering bridge nodes.
pub struct ShareApi {
    ctx: Arc<Context>,
}

impl ShareApi {
    pub(crate) fn new(ctx: Arc<Context>) -> ShareApi {
        ShareApi { ctx }
    }

    /// Performs a subjective validation to check if the shares committed to the
    /// ExtendedHeader at the specified height are available and retrievable from
    /// the network.
    ///
    /// Returns `Ok(())` if shares are available.
    pub async fn shares_available(&self, height: u64) -> Result<()> {
        Ok(self.ctx.rpc.share_shares_available(height).await?)
    }

    /// Retrieves a specific share from the [`ExtendedDataSquare`] at the given
    /// height  using its row and column coordinates.
    pub async fn get(&self, height: u64, row: u64, column: u64) -> Result<Share> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_share(&header, row, column).await?)
    }

    /// Retrieves multiple shares from the [`ExtendedDataSquare`] at the given
    /// sample coordinates.
    ///
    /// `coordinates` is a list of `(row, column)`.
    pub async fn get_samples(
        &self,
        height: u64,
        coordinates: &[SampleCoordinates],
    ) -> Result<Vec<Sample>> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_samples(&header, coordinates).await?)
    }

    /// Retrieves the complete [`ExtendedDataSquare`] for the specified height.
    pub async fn get_eds(&self, height: u64) -> Result<ExtendedDataSquare> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_eds(&header).await?)
    }

    /// Retrieves all shares from a specific row of the [`ExtendedDataSquare`]
    /// at the given height.
    pub async fn get_row(&self, height: u64, row: u64) -> Result<GetRowResponse> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_row(&header, row).await?)
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
        let header = self.ctx.get_header_validated(height).await?;

        Ok(self
            .ctx
            .rpc
            .share_get_namespace_data(&header, namespace)
            .await?)
    }

    /// Retrieves a list of shares and their corresponding proof.
    ///
    /// The start and end index ignores parity shares and corresponds to ODS.
    pub async fn get_range(&self, height: u64, start: u64, end: u64) -> Result<GetRangeResponse> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_range(&header, start, end).await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::Client;

    use super::*;

    #[tokio::test]
    async fn shares_available() {
        let client = Client::builder()
            .rpc_url("ws://localhost:46658")
            .build()
            .await
            .unwrap();

        client.share().shares_available(3092).await.unwrap();
    }
}
