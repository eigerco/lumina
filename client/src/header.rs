use std::sync::Arc;

use celestia_rpc::HeaderClient;
use celestia_types::hash::Hash;
use celestia_types::{ExtendedHeader, SyncState};
use futures_util::{Stream, StreamExt};

use crate::client::Context;
use crate::Result;

/// Header API for quering bridge nodes.
pub struct HeaderApi {
    ctx: Arc<Context>,
}

impl HeaderApi {
    pub(crate) fn new(ctx: Arc<Context>) -> HeaderApi {
        HeaderApi { ctx }
    }

    /// Returns the head from the node's header store.
    // TODO: keep this name or `local_head`?
    pub async fn head(&self) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_local_head().await?;
        header.validate()?;
        Ok(header)
    }

    /// Provides the Syncer's view of the current network head.
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

    /// Returns the [`ExtendedHeader`] at the given height if it is
    /// currently available.
    pub async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let header = self.ctx.rpc.header_get_by_height(height).await?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the given range of [`ExtendedHeader`]s from the node's
    /// header store and verifies that the returned headers are adjacent
    /// to each other.
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

    /// Blocks until the header at the given height has been processed by the store.
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

    /// Subscribe to recent ExtendedHeaders from the network.
    pub async fn subscribe(&self) -> Result<impl Stream<Item = Result<ExtendedHeader>>> {
        Ok(self.ctx.rpc.header_subscribe().await?.map(|item| {
            item.map_err(Into::into).and_then(|header| {
                header.validate()?;
                Ok(header)
            })
        }))
    }
}
