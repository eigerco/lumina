use std::pin::Pin;
use std::sync::Arc;

use celestia_rpc::{Error as CelestiaRpcError, FraudClient};
use futures_util::Stream;
use jsonrpsee_core::ClientError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::api::fraud::{Proof, ProofType};
use crate::client::ClientInner;
use crate::{Error, Result};

use crate::exec_rpc;

/// Fraud API for quering bridge nodes.
pub struct FraudApi {
    inner: Arc<ClientInner>,
}

impl FraudApi {
    pub(crate) fn new(inner: Arc<ClientInner>) -> FraudApi {
        FraudApi { inner }
    }

    /// Fetches fraud proofs from node by their type.
    pub async fn get(&self, proof_type: ProofType) -> Result<Vec<Proof>> {
        exec_rpc!(self, |rpc| async {
            rpc.fraud_get(proof_type)
                .await
                .map_err(CelestiaRpcError::from)
        })
    }

    /// Subscribe to fraud proof by its type.
    ///
    /// This method is resilient to network interruptions and will automatically
    /// attempt to reconnect and re-subscribe if the connection is lost.
    pub async fn subscribe(
        &self,
        proof_type: ProofType,
    ) -> Pin<Box<dyn Stream<Item = Result<Proof>> + Send + 'static>> {
        let (tx, rx) = mpsc::channel(128);
        let rpc_manager = self.inner.rpc_manager.clone();

        // Spawn a separate task to manage the subscription's lifecycle. This isolates
        // the connection and allows it to transparently reconnect in the background.
        tokio::spawn(async move {
            'reconnect: loop {
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
                            log::error!("Failed to create fraud subscription client: {e}. Retrying in 5s...");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue 'reconnect;
                        }
                    }
                };

                let mut subscription = match client.fraud_subscribe(proof_type).await {
                    Ok(sub) => {
                        log::info!("Successfully subscribed to fraud proof stream.");
                        sub
                    }
                    Err(e) => {
                        log::warn!("Failed to subscribe to fraud proof stream: {e}. Retrying...");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue 'reconnect;
                    }
                };

                loop {
                    let is_connected = match &client {
                        celestia_rpc::Client::Ws(ws) => ws.is_connected(),
                        celestia_rpc::Client::Http(_) => true,
                    };

                    if !is_connected {
                        log::warn!("Fraud subscription client disconnected. Reconnecting...");
                        continue 'reconnect;
                    }

                    match subscription.next().await {
                        Some(Ok(proof)) => {
                            // If `send` fails, the stream's receiver has been dropped,
                            // so we can gracefully terminate this task.
                            if tx.send(Ok(proof)).await.is_err() {
                                log::info!("Fraud subscription receiver dropped, ending task.");
                                return;
                            }
                        }
                        Some(Err(e)) => {
                            log::warn!("Error on fraud subscription stream: {e}. Re-subscribing.");
                            let err =
                                Error::from(CelestiaRpcError::from(ClientError::ParseError(e)));
                            if tx.send(Err(err)).await.is_err() {
                                return;
                            }

                            continue 'reconnect;
                        }
                        None => {
                            // A `None` from the stream means the server closed the connection.
                            log::warn!(
                                "Fraud subscription stream closed by server. Re-subscribing."
                            );
                            continue 'reconnect;
                        }
                    }
                }
            }
        });

        let receiver_stream = ReceiverStream::new(rx);
        Box::pin(receiver_stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::ensure_serializable_deserializable;
    use futures_util::StreamExt;

    #[allow(dead_code)]
    #[allow(unused_variables)]
    #[allow(unreachable_code)]
    #[allow(clippy::diverging_sub_expression)]
    async fn enforce_serde_bounds() {
        // intentionally no-run, compile only test
        let api = FraudApi::new(unimplemented!());

        let proof_type = ensure_serializable_deserializable(unimplemented!());
        ensure_serializable_deserializable(api.get(proof_type).await.unwrap());

        let proof_type = ensure_serializable_deserializable(unimplemented!());
        ensure_serializable_deserializable(
            api.subscribe(proof_type)
                .await
                .next()
                .await
                .unwrap()
                .unwrap(),
        );
    }
}
