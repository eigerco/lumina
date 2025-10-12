use std::sync::Arc;
use std::time::Duration;

use celestia_rpc::Client as RpcClient;

use tokio::sync::{watch, Notify, RwLock};
use tokio_retry::strategy::{jitter, ExponentialBackoff};
use tokio_retry::Retry;

use crate::Result;

const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const LONG_RETRY_DELAY: Duration = Duration::from_secs(60);

pub type ShutdownHandle = watch::Sender<bool>;

/// Manages the RPC client's lifecycle, providing a resilient connection
/// that automatically recovers from network interruptions.
#[derive(Clone)]
pub struct RpcConnectionManager {
    pub(crate) client_lock: Arc<RwLock<Option<RpcClient>>>,
    pub(crate) notify_new_client: Arc<Notify>,
    pub rpc_url: String,
    pub rpc_auth_token: Option<String>,
    // The sending-half of the shutdown channel.
    shutdown_tx: Arc<ShutdownHandle>,
}

impl RpcConnectionManager {
    /// Creates a new RpcConnectionManager and spawns its background management task.
    pub async fn new(rpc_url: &str, rpc_auth_token: Option<&str>) -> Result<Arc<Self>> {
        let initial_client = Self::try_connect_with_retry(rpc_url, rpc_auth_token).await?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let manager = Arc::new(RpcConnectionManager {
            client_lock: Arc::new(RwLock::new(Some(initial_client))),
            notify_new_client: Arc::new(Notify::new()),
            rpc_url: rpc_url.to_string(),
            rpc_auth_token: rpc_auth_token.map(String::from),
            shutdown_tx: Arc::new(shutdown_tx),
        });

        tokio::spawn(Arc::clone(&manager).connection_loop(shutdown_rx));
        Ok(manager)
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        (*self.shutdown_tx).clone()
    }

    // The core background task that monitors connection health and handles reconnection.
    async fn connection_loop(self: Arc<Self>, mut shutdown_rx: watch::Receiver<bool>) {
        loop {
            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {
                    log::info!("Shutdown signal received, terminating connection manager.");
                    break;
                }
                _ = tokio::time::sleep(HEALTH_CHECK_INTERVAL) => {
                }
            }

            let is_connected = {
                let guard = self.client_lock.read().await;
                guard.as_ref().map_or(false, |client| match client {
                    RpcClient::Ws(ws_client) => ws_client.is_connected(),
                    RpcClient::Http(_) => true, // HTTP clients are stateless.
                })
            };

            if !is_connected {
                log::warn!("WebSocket connection lost. Attempting to reconnect...");
                self.client_lock.write().await.take();

                // Race the reconnect attempt against the shutdown signal.
                let reconnect_result = tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        log::info!("Shutdown signal received during reconnect, terminating.");
                        break;
                    }
                    result = Self::try_connect_with_retry(&self.rpc_url, self.rpc_auth_token.as_deref()) => {
                        result
                    }
                };

                match reconnect_result {
                    Ok(new_client) => {
                        let mut guard = self.client_lock.write().await;
                        *guard = Some(new_client);
                        log::info!("Successfully reconnected to RPC endpoint.");
                        self.notify_new_client.notify_waiters();
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to reconnect after multiple retries: {}. Retrying after {} seconds.",
                            e, LONG_RETRY_DELAY.as_secs()
                        );
                        // Also race the long sleep against the shutdown signal.
                        tokio::select! {
                            biased;
                            _ = shutdown_rx.changed() => {
                                log::info!("Shutdown signal received during retry delay, terminating.");
                                break;
                            }
                            _ = tokio::time::sleep(LONG_RETRY_DELAY) => {}
                        }
                    }
                }
            }
        }
    }

    // Connects to the RPC endpoint with an exponential backoff retry strategy.
    async fn try_connect_with_retry(url: &str, token: Option<&str>) -> Result<RpcClient> {
        let retry_strategy = ExponentialBackoff::from_millis(100).map(jitter).take(5);

        let action = || async {
            log::debug!("Attempting to connect to RPC endpoint: {}", url);
            #[cfg(not(target_arch = "wasm32"))]
            let client = RpcClient::new(url, token).await.map_err(|e| {
                log::warn!("Connection attempt failed: {}", e);
                e
            })?;

            #[cfg(target_arch = "wasm32")]
            let client = RpcClient::new(url).await.map_err(|e| {
                log::warn!("Connection attempt failed: {}", e);
                e
            })?;

            Ok(client)
        };

        Retry::spawn(retry_strategy, action)
            .await
            .map_err(|e: celestia_rpc::Error| e.into())
    }
}
