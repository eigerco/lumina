use std::sync::Arc;

use celestia_rpc::{
    share::{GetRangeResponse, GetRowResponse},
    HeaderClient, ShareClient,
};
use celestia_types::{
    nmt::Namespace, row_namespace_data::NamespaceData, ExtendedDataSquare, Height, Share,
};

use crate::client::Context;
use crate::Result;

pub struct ShareApi {
    ctx: Arc<Context>,
}

impl ShareApi {
    pub(crate) fn new(ctx: Arc<Context>) -> ShareApi {
        ShareApi { ctx }
    }

    /*
     type Module interface {
    // SharesAvailable performs a subjective validation to check if the shares committed to
    // the ExtendedHeader at the specified height are available and retrievable from the network.
    // Returns an error if the shares are not available or if validation fails.
    SharesAvailable(ctx context.Context, height uint64) error

    // GetShare retrieves a specific share from the Extended Data Square (EDS) at the given height
    // using its row and column coordinates. Returns the share data or an error if retrieval fails.
    GetShare(ctx context.Context, height uint64, rowIdx, colIdx int) (libshare.Share, error)

    // GetSamples retrieves multiple shares from the Extended Data Square (EDS) specified by the header
    // at the given sample coordinates. Returns an array of samples containing the requested shares
    // or an error if retrieval fails.
    GetSamples(ctx context.Context, header *header.ExtendedHeader, indices []shwap.SampleCoords) ([]shwap.Sample, error)

    // GetEDS retrieves the complete Extended Data Square (EDS) for the specified height.
    // The EDS contains all shares organized in a 2D matrix format with erasure coding.
    // Returns the full EDS or an error if retrieval fails.
    GetEDS(ctx context.Context, height uint64) (*rsmt2d.ExtendedDataSquare, error)

    // GetRow retrieves all shares from a specific row in the Extended Data Square (EDS)
    // at the given height. Returns the complete row of shares or an error if retrieval fails.
    GetRow(ctx context.Context, height uint64, rowIdx int) (shwap.Row, error)

    // GetNamespaceData retrieves all shares that belong to the specified namespace within
    // the Extended Data Square (EDS) at the given height. The shares are returned in a
    // row-by-row order, maintaining the original layout if the namespace spans multiple rows.
    // Returns the namespace data or an error if retrieval fails.
    GetNamespaceData(
        ctx context.Context,
        height uint64,
        namespace libshare.Namespace,
    ) (shwap.NamespaceData, error)

    // GetRange retrieves a range of shares and their corresponding proofs within a specific
    // namespace in the Extended Data Square (EDS) at the given height. The range is defined
    // by from and to coordinates. If proofsOnly is true, only the proofs are returned without
    // the actual share data. Returns the range data with proofs or an error if retrieval fails.
    GetRange(
        ctx context.Context,
        namespace libshare.Namespace,
        height uint64,
        fromCoords, toCoords shwap.SampleCoords,
        proofsOnly bool,
    ) (shwap.RangeNamespaceData, error)
    */

    /// Performs a subjective validation to check if the shares committed to the
    /// ExtendedHeader at the specified height are available and retrievable from the network.
    ///
    /// Returns `Ok(())` if shares are available.
    // TODO: It should return `Result<bool>` and error only on jsonrpsee errors
    pub async fn shares_available(&self, height: u64) -> Result<()> {
        Ok(self.ctx.rpc.share_shares_available(height).await?)
    }

    pub async fn get(&self, height: u64, row: u64, column: u64) -> Result<Share> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_share(&header, row, column).await?)
    }

    // TODO pub async fn get_samples(&self

    pub async fn get_eds(&self, height: u64) -> Result<ExtendedDataSquare> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_eds(&header).await?)
    }

    pub async fn get_row(&self, height: u64, row: u64) -> Result<GetRowResponse> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_row(&header, row).await?)
    }

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
    pub async fn get_range(&self, height: u64, start: u64, end: u64) -> Result<GetRangeResponse> {
        let header = self.ctx.get_header_validated(height).await?;
        Ok(self.ctx.rpc.share_get_range(&header, start, end).await?)
    }
}
