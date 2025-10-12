use std::pin::Pin;
use std::sync::Arc;

use async_stream::try_stream;
use celestia_rpc::{Error as CelestiaRpcError, HeaderClient};
use futures_util::{Stream, StreamExt};
use jsonrpsee_core::ClientError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::client::ClientInner;
use crate::types::hash::Hash;
use crate::types::{ExtendedHeader, SyncState};
use crate::{Error, Result};

use crate::exec_rpc;

/// Header API for quering bridge nodes.
pub struct HeaderApi {
    inner: Arc<ClientInner>,
}

impl HeaderApi {
    pub(crate) fn new(inner: Arc<ClientInner>) -> HeaderApi {
        HeaderApi { inner }
    }

    /// Returns the latest header synchronized by the node.
    pub async fn head(&self) -> Result<ExtendedHeader> {
        let header = exec_rpc!(self, |rpc| async {
            rpc.header_local_head()
                .await
                .map_err(CelestiaRpcError::from)
        })?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the latest header announced in the network.
    pub async fn network_head(&self) -> Result<ExtendedHeader> {
        let header = exec_rpc!(self, |rpc| async {
            rpc.header_network_head()
                .await
                .map_err(CelestiaRpcError::from)
        })?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the header of the given hash from the node's header store.
    pub async fn get_by_hash(&self, hash: Hash) -> Result<ExtendedHeader> {
        let header = exec_rpc!(self, |rpc| async {
            rpc.header_get_by_hash(hash)
                .await
                .map_err(CelestiaRpcError::from)
        })?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the header at the given height, if it is
    /// currently available.
    pub async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let header = exec_rpc!(self, |rpc| async {
            rpc.header_get_by_height(height)
                .await
                .map_err(CelestiaRpcError::from)
        })?;
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

        let headers = exec_rpc!(self, |rpc| async {
            rpc.header_get_range_by_height(from, to)
                .await
                .map_err(CelestiaRpcError::from)
        })?;

        for header in &headers {
            header.validate()?;
        }

        from.verify_adjacent_range(&headers)?;

        Ok(headers)
    }

    /// Blocks until the header at the given height has been synced by the node.
    pub async fn wait_for_height(&self, height: u64) -> Result<ExtendedHeader> {
        let header = exec_rpc!(self, |rpc| async {
            rpc.header_wait_for_height(height)
                .await
                .map_err(CelestiaRpcError::from)
        })?;
        header.validate()?;
        Ok(header)
    }

    /// Returns the current state of the node's Syncer.
    pub async fn sync_state(&self) -> Result<SyncState> {
        let state = exec_rpc!(self, |rpc| async {
            rpc.header_sync_state()
                .await
                .map_err(CelestiaRpcError::from)
        })?;
        Ok(state)
    }

    /// Blocks until the node's Syncer is synced to network head.
    pub async fn sync_wait(&self) -> Result<()> {
        exec_rpc!(self, |rpc| async {
            rpc.header_sync_wait().await.map_err(CelestiaRpcError::from)
        })?;
        Ok(())
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
    /// # async fn docs() -> Result<()> {
    /// let client = Client::builder()
    ///     .rpc_url("ws://localhost:26658")
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
        let (tx, rx) = mpsc::channel(128);
        let rpc_manager = self.inner.rpc_manager.clone();

        // Spawn a separate task to manage the subscription's lifecycle.
        // This isolates the connection and allows it to transparently reconnect in the background without disrupting the consumer of the stream.
        tokio::spawn(async move {
            'reconnect: loop {
                // Create a new, dedicated client for each connection attempt.
                let client = {
                    #[cfg(not(target_arch = "wasm32"))]
                    let client_result = celestia_rpc::Client::new(
                        &rpc_manager.rpc_url,
                        rpc_manager.rpc_auth_token.as_deref(),
                    )
                    .await;
                    #[cfg(target_arch = "wasm32")]
                    let client_result = celestia_rpc::Client::new(&rpc_manager.rpc_url).await;

                    match client_result {
                        Ok(c) => c,
                        Err(e) => {
                            log::error!(
                                "Failed to create subscription client: {e}. Retrying in 5s..."
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue 'reconnect;
                        }
                    }
                };

                let mut subscription = match client.header_subscribe().await {
                    Ok(sub) => {
                        log::info!("Successfully subscribed to header stream.");
                        sub
                    }
                    Err(e) => {
                        log::warn!("Failed to subscribe to header stream: {e}. Retrying...");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue 'reconnect;
                    }
                };

                loop {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let celestia_rpc::Client::Ws(ws) = &client {
                        if !ws.is_connected() {
                            log::warn!("Subscription client disconnected. Reconnecting...");
                            continue 'reconnect;
                        }
                    }

                    match subscription.next().await {
                        Some(Ok(header)) => {
                            // If 'send' fails, the receiver has been dropped, so we can gracefully terminate the background task.
                            if tx.send(Ok(header)).await.is_err() {
                                log::info!("Subscription receiver dropped, ending task.");
                                return;
                            }
                        }
                        Some(Err(e)) => {
                            log::warn!("Error on header subscription stream: {e}. Re-subscribing.");
                            let err =
                                Error::from(CelestiaRpcError::from(ClientError::ParseError(e)));
                            if tx.send(Err(err)).await.is_err() {
                                return; // Receiver dropped.
                            }
                            continue 'reconnect;
                        }
                        None => {
                            // A 'None' from the stream means the server closed the connection.
                            log::warn!(
                                "Header subscription stream closed by server. Re-subscribing."
                            );
                            continue 'reconnect;
                        }
                    }
                }
            }
        });

        // The stream returned to the user is fed by the background task. It also applies adjacent header verification.
        let receiver_stream = ReceiverStream::new(rx);

        let validated_stream = try_stream! {
            let mut prev_header: Option<ExtendedHeader> = None;

            for await item_result in receiver_stream {
                let header = item_result?;
                header.validate()?;

                if let Some(ref prev_header) = prev_header {
                    prev_header.verify_adjacent(&header)?;
                }

                prev_header = Some(header.clone());
                yield header;
            }
        };

        validated_stream.boxed()
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::client::ClientInner;
    use crate::connection_manager::RpcConnectionManager;
    use crate::test_utils::ensure_serializable_deserializable;
    use futures_util::StreamExt;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

    const TEST_WS_URL: &str = "ws://localhost:26658";

    /// Creates a `HeaderApi` instance connected to a live dev node for integration tests.
    async fn setup_test_api() -> HeaderApi {
        let rpc_manager = RpcConnectionManager::new(TEST_WS_URL, None)
            .await
            .expect("Failed to create RpcConnectionManager. Is a local dev node running?");

        let chain_id = "private".parse().unwrap();
        let client_inner = Arc::new(ClientInner::new_for_test(rpc_manager, chain_id));
        HeaderApi::new(client_inner)
    }

    /// A smoke test for one-shot RPC calls.
    #[tokio::test]
    #[ignore = "requires a running Celestia dev node"]
    async fn get_network_head_from_node() {
        let api = setup_test_api().await;

        let head = api
            .network_head()
            .await
            .expect("Failed to get network head from a running node.");

        assert!(head.height().value() > 0, "Node should have a height > 0");
    }

    /// Verifies that the subscription stream can receive a new header.
    #[tokio::test]
    #[ignore = "requires a running Celestia dev node"]
    async fn subscribe_to_headers_from_node() {
        let api = setup_test_api().await;
        let mut sub_stream = api.subscribe().await;

        // A 10-second timeout is ample for a local dev node to produce a block.
        match timeout(Duration::from_secs(10), sub_stream.next()).await {
            Ok(Some(Ok(header))) => {
                assert!(header.height().value() > 0);
            }
            Ok(Some(Err(e))) => panic!("Subscription stream returned an error: {:?}", e),
            Ok(None) => panic!("Subscription stream closed unexpectedly."),
            Err(_) => panic!("Timed out waiting for a new header from the subscription stream."),
        }
    }

    /// A compile-only test to ensure API return types remain serializable.
    #[test]
    #[ignore = "compile-only test for serde bounds"]
    fn compile_only_serde_bounds_check() {
        #[allow(
            dead_code,
            unused_variables,
            unreachable_code,
            clippy::diverging_sub_expression
        )]
        async fn enforce_serde_bounds() {
            let api = HeaderApi::new(unimplemented!());

            let _: () = api.sync_wait().await.unwrap();

            ensure_serializable_deserializable(api.head().await.unwrap());

            ensure_serializable_deserializable(api.network_head().await.unwrap());

            let hash = ensure_serializable_deserializable(unimplemented!());
            ensure_serializable_deserializable(api.get_by_hash(hash).await.unwrap());

            ensure_serializable_deserializable(api.get_by_height(0).await.unwrap());

            let header = ensure_serializable_deserializable(unimplemented!());
            ensure_serializable_deserializable(api.get_range_by_height(&header, 0).await.unwrap());

            ensure_serializable_deserializable(api.wait_for_height(0).await.unwrap());

            ensure_serializable_deserializable(api.sync_state().await.unwrap());
            ensure_serializable_deserializable(
                api.subscribe().await.next().await.unwrap().unwrap(),
            );
        }
    }
}
