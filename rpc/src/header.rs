use std::future::Future;
use std::marker::{Send, Sync};
use std::pin::Pin;

use async_stream::try_stream;
use celestia_types::hash::Hash;
use celestia_types::{ExtendedHeader, SyncState};
use futures_util::{Stream, StreamExt};
use jsonrpsee::core::client::{ClientT, Error, SubscriptionClientT};
use jsonrpsee::proc_macros::rpc;

use crate::custom_client_error;

mod rpc {
    use super::*;

    #[rpc(client, namespace = "header", namespace_separator = ".")]
    pub trait Header {
        #[method(name = "GetByHash")]
        async fn header_get_by_hash(&self, hash: Hash) -> Result<ExtendedHeader, Error>;

        #[method(name = "GetByHeight")]
        async fn header_get_by_height(&self, height: u64) -> Result<ExtendedHeader, Error>;

        #[method(name = "GetRangeByHeight")]
        async fn header_get_range_by_height(
            &self,
            from: &ExtendedHeader,
            to: u64,
        ) -> Result<Vec<ExtendedHeader>, Error>;

        #[method(name = "LocalHead")]
        async fn header_local_head(&self) -> Result<ExtendedHeader, Error>;

        #[method(name = "NetworkHead")]
        async fn header_network_head(&self) -> Result<ExtendedHeader, Error>;

        #[method(name = "SyncState")]
        async fn header_sync_state(&self) -> Result<SyncState, Error>;

        #[method(name = "SyncWait")]
        async fn header_sync_wait(&self) -> Result<(), Error>;

        #[method(name = "WaitForHeight")]
        async fn header_wait_for_height(&self, height: u64) -> Result<ExtendedHeader, Error>;
    }

    #[rpc(client, namespace = "header", namespace_separator = ".")]
    pub trait HeaderSubscription {
        #[subscription(name = "Subscribe", unsubscribe = "Unsubscribe", item = ExtendedHeader)]
        async fn header_subscribe(&self) -> SubscriptionResult;
    }
}

/// Client implementation for the `Header` RPC API.
pub trait HeaderClient: ClientT {
    /// GetByHash returns the header of the given hash from the node's header store.
    fn header_get_by_hash<'a, 'fut>(
        &'a self,
        hash: Hash,
    ) -> impl Future<Output = Result<ExtendedHeader, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_get_by_hash(self, hash)
    }

    /// GetByHeight returns the ExtendedHeader at the given height if it is currently available.
    fn header_get_by_height<'a, 'fut>(
        &'a self,
        height: u64,
    ) -> impl Future<Output = Result<ExtendedHeader, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_get_by_height(self, height)
    }

    /// GetRangeByHeight returns the given range (from:to) of ExtendedHeaders from the node's header store and verifies that the returned headers are adjacent to each other.
    fn header_get_range_by_height<'a, 'b, 'fut>(
        &'a self,
        from: &'b ExtendedHeader,
        to: u64,
    ) -> impl Future<Output = Result<Vec<ExtendedHeader>, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_get_range_by_height(self, from, to)
    }

    /// LocalHead returns the ExtendedHeader of the chain head.
    fn header_local_head<'a, 'fut>(
        &'a self,
    ) -> impl Future<Output = Result<ExtendedHeader, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_local_head(self)
    }

    /// NetworkHead provides the Syncer's view of the current network head.
    fn header_network_head<'a, 'fut>(
        &'a self,
    ) -> impl Future<Output = Result<ExtendedHeader, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_network_head(self)
    }

    /// Subscribe to recent ExtendedHeaders from the network.
    ///
    /// # Notes
    ///
    /// If client returns [`Error::HttpNotImplemented`], the subscription will fallback to
    /// using [`HeaderClient::header_wait_for_height`] for streaming the headers.
    fn header_subscribe<'a>(
        &'a self,
    ) -> Pin<Box<dyn Stream<Item = Result<ExtendedHeader, Error>> + Send + 'a>>
    where
        Self: SubscriptionClientT + Sized + Sync,
    {
        try_stream! {
            let mut head = rpc::HeaderClient::header_local_head(self).await?;

            let subscription_res = rpc::HeaderSubscriptionClient::header_subscribe(self).await;
            let has_real_sub = !matches!(&subscription_res, Err(Error::HttpNotImplemented));

            let mut subscription = if has_real_sub {
                Some(subscription_res?)
            } else {
                None
            };

            loop {
                let header = if has_real_sub {
                    subscription
                        .as_mut()
                        .expect("must be some")
                        .next()
                        .await
                        .ok_or_else(|| custom_client_error("unexpected end of stream"))??
                } else {
                    rpc::HeaderClient::header_wait_for_height(self, head.height().value()).await?
                };

                header.validate().map_err(custom_client_error)?;
                head.verify_adjacent(&header).map_err(custom_client_error)?;

                head = header.clone();
                yield header;
            }
        }
        .boxed()
    }

    /// SyncState returns the current state of the header Syncer.
    fn header_sync_state<'a, 'fut>(
        &'a self,
    ) -> impl Future<Output = Result<SyncState, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_sync_state(self)
    }

    /// SyncWait blocks until the header Syncer is synced to network head.
    fn header_sync_wait<'a, 'fut>(&'a self) -> impl Future<Output = Result<(), Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_sync_wait(self)
    }

    /// WaitForHeight blocks until the header at the given height has been processed by the store or context deadline is exceeded.
    fn header_wait_for_height<'a, 'fut>(
        &'a self,
        height: u64,
    ) -> impl Future<Output = Result<ExtendedHeader, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::HeaderClient::header_wait_for_height(self, height)
    }
}

impl<T> HeaderClient for T where T: ClientT {}
