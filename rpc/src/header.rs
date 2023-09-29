use celestia_types::hash::Hash;
use celestia_types::{ExtendedHeader, SyncState};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Header {
    /// GetByHash returns the header of the given hash from the node's header store.
    #[method(name = "header.GetByHash")]
    async fn header_get_by_hash(&self, hash: Hash) -> Result<ExtendedHeader, Error>;

    /// GetByHeight returns the ExtendedHeader at the given height if it is currently available.
    #[method(name = "header.GetByHeight")]
    async fn header_get_by_height(&self, height: u64) -> Result<ExtendedHeader, Error>;

    /// GetVerifiedRangeByHeight returns the given range (from:to) of ExtendedHeaders from the node's header store and verifies that the returned headers are adjacent to each other.
    #[method(name = "header.GetVerifiedRangeByHeight")]
    async fn header_get_verified_range_by_height(
        &self,
        from: &ExtendedHeader,
        to: u64,
    ) -> Result<Vec<ExtendedHeader>, Error>;

    /// LocalHead returns the ExtendedHeader of the chain head.
    #[method(name = "header.LocalHead")]
    async fn header_local_head(&self) -> Result<ExtendedHeader, Error>;

    /// NetworkHead provides the Syncer's view of the current network head.
    #[method(name = "header.NetworkHead")]
    async fn header_network_head(&self) -> Result<ExtendedHeader, Error>;

    /// Subscribe to recent ExtendedHeaders from the network.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[subscription(name = "header.Subscribe", unsubscribe = "header.Unsubscribe", item = ExtendedHeader)]
    async fn header_subscribe(&self) -> SubcriptionResult;

    /// SyncState returns the current state of the header Syncer.
    #[method(name = "header.SyncState")]
    async fn header_sync_state(&self) -> Result<SyncState, Error>;

    /// SyncWait blocks until the header Syncer is synced to network head.
    #[method(name = "header.SyncWait")]
    async fn header_sync_wait(&self) -> Result<(), Error>;

    /// WaitForHeight blocks until the header at the given height has been processed by the store or context deadline is exceeded.
    #[method(name = "header.WaitForHeight")]
    async fn header_wait_for_height(&self, height: u64) -> Result<ExtendedHeader, Error>;
}
