use std::pin::Pin;
use std::sync::Arc;

use async_stream::try_stream;
use celestia_rpc::HeaderClient;
use futures_util::{Stream, StreamExt};

use crate::client::Context;
use crate::types::hash::Hash;
use crate::types::{ExtendedHeader, SyncState};
use crate::Result;

/// Header API for quering bridge nodes.
pub struct HeaderApi {
    ctx: Arc<Context>,
}

impl HeaderApi {
    pub(crate) fn new(ctx: Arc<Context>) -> HeaderApi {
        HeaderApi { ctx }
    }

    /// Returns the latest head header synchronized by the node.
    pub async fn head(&self) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_local_head().await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the latest head header announced in the network.
    pub async fn network_head(&self) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_network_head().await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the header of the given hash from the node's header store.
    pub async fn get_by_hash(&self, hash: Hash) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_get_by_hash(hash).await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the header at the given height, if it is
    /// currently available.
    pub async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_get_by_height(height).await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the given range headers from the node and verifies that they
    /// form a confirmed and continuous chain starting from the `from` header.
    ///
    /// The range is exclusive from both sides.
    pub async fn get_range_by_height(
        &self,
        from: &ExtendedHeader,
        to: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        from.validate()?;

        let headers = self.ctx.rpc.header_get_range_by_height(from, to).await?;

        for header in &headers {
            header.validate()?;
        }

        from.verify_adjacent_range(&headers)?;

        Ok(headers)
    }

    /// Blocks until the header at the given height has been synced by the node.
    pub async fn wait_for_height(&self, height: u64) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_wait_for_height(height).await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the current state of the node's Syncer.
    pub async fn sync_state(&self) -> Result<SyncState> {
        Ok(self.ctx.rpc.header_sync_state().await?)
    }

    /// Blocks until the node's Syncer is synced to network head.
    pub async fn sync_wait(&self) -> Result<()> {
        Ok(self.ctx.rpc.header_sync_wait().await?)
    }

    /// Subscribe to recent headers from the network.
    ///
    /// Headers will be validated and verified with the one that was
    /// previously received.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use futures_util::StreamExt;
    /// # use celestia_client::{Client, Result};
    /// # const RPC_URL: &str = "ws://localhost:26658";
    /// # async fn docs() -> Result<()> {
    /// let client = Client::builder()
    ///     .rpc_url(RPC_URL)
    ///     .build()
    ///     .await?;
    ///
    /// let mut headers_rx = client.header().subscribe().await;
    ///
    /// while let Some(header) = headers_rx.next().await {
    ///     dbg!(header);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<ExtendedHeader>> + Send + 'static>> {
        let ctx = self.ctx.clone();

        try_stream! {
            let mut prev_header: Option<ExtendedHeader> = None;
            let mut subscription = ctx.rpc.header_subscribe().await?;

            while let Some(item) = subscription.next().await {
                let header = item?;
                header.validate()?;

                if let Some(ref prev_header) = prev_header {
                    prev_header.verify_adjacent(&header)?;
                }

                prev_header = Some(header.clone());
                yield header;
            }
        }
        .boxed()
    }
}
