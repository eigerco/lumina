use std::sync::Arc;
use std::time::Duration;

use celestia_types::blob::BlobsAtHeight;
use celestia_types::nmt::Namespace;
use celestia_types::{ExtendedHeader, SharesAtHeight};
use tokio::sync::mpsc;

use crate::NodeError;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};

const SHWAP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Error thrown while processing the subscription
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Unable to receive subscription item at {height}: {source}")]
    Node {
        /// Height of the subscription item
        height: u64,
        /// Error that occured
        #[source]
        source: NodeError,
    },
    #[error("Subscription item height already pruned from the store, skipping {0} items")]
    Lagged(u64),
}

pub(crate) async fn forward_new_headers<S: Store>(
    store: Arc<S>,
    tx: mpsc::Sender<Result<ExtendedHeader, SubscriptionError>>,
) {
    let mut head = store.wait_new_head().await;
    let mut height = head;

    loop {
        while height <= head {
            let header_or_error = match store.get_by_height(height).await {
                Ok(h) => Ok(h),
                Err(StoreError::NotFound) => {
                    // head already was pruned, restart the subscription and hope
                    // the client can keep up this time
                    let current_head = store.wait_new_head().await;
                    let skipped = current_head.saturating_sub(height);
                    height = current_head - 1; // cancel out the addition below
                    Err(SubscriptionError::Lagged(skipped))
                }
                Err(e) => Err(SubscriptionError::Node {
                    height,
                    source: NodeError::Store(e),
                }),
            };
            if tx.send(header_or_error).await.is_err() {
                return; // receiver dropped
            }
            height += 1;
        }

        head = store.wait_new_head().await;
    }
}

pub(crate) async fn forward_new_blobs<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<BlobsAtHeight, SubscriptionError>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut head = store.wait_new_head().await;
    let mut height = head;

    loop {
        while height <= head {
            let blobs_or_error = match p2p
                .get_all_blobs(namespace, height, Some(SHWAP_FETCH_TIMEOUT), store.as_ref())
                .await
            {
                Ok(blobs) => Ok(BlobsAtHeight { height, blobs }),
                Err(P2pError::Store(StoreError::NotFound)) => {
                    // head already was pruned, restart the subscription and hope
                    // the client can keep up this time
                    let current_head = store.wait_new_head().await;
                    let skipped = current_head.saturating_sub(height);
                    height = current_head - 1; // cancel out the addition below
                    Err(SubscriptionError::Lagged(skipped))
                }
                Err(e) => Err(SubscriptionError::Node {
                    height,
                    source: NodeError::P2p(e),
                }),
            };
            if tx.send(blobs_or_error).await.is_err() {
                return; // receiver dropped
            }
            height += 1;
        }
        head = store.wait_new_head().await;
    }
}

pub(crate) async fn forward_new_shares<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<SharesAtHeight, SubscriptionError>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut head = store.wait_new_head().await;
    let mut height = head;

    loop {
        while height <= head {
            let header_or_error = match store.get_by_height(height).await {
                Ok(h) => Ok(h),
                Err(StoreError::NotFound) => {
                    // head already was pruned, restart the subscription and hope
                    // the client can keep up this time
                    let current_head = store.wait_new_head().await;
                    let skipped = current_head.saturating_sub(height);
                    height = current_head - 1; // cancel out the addition below
                    Err(SubscriptionError::Lagged(skipped))
                }
                Err(e) => Err(SubscriptionError::Node {
                    height,
                    source: NodeError::Store(e),
                }),
            };
            if let Err(e) = header_or_error {
                if tx.send(Err(e)).await.is_err() {
                    return;
                }
                continue;
            }
            let header = header_or_error.unwrap();

            let shares_or_error = p2p
                .get_namespace_data(
                    namespace,
                    &header,
                    Some(SHWAP_FETCH_TIMEOUT),
                    store.as_ref(),
                )
                .await
                .map(|data| SharesAtHeight {
                    height,
                    shares: data
                        .rows
                        .into_iter()
                        .flat_map(|row| row.shares.into_iter())
                        .collect(),
                })
                .map_err(|e| SubscriptionError::Node {
                    height,
                    source: NodeError::P2p(e),
                });
            if tx.send(shares_or_error).await.is_err() {
                return; // receiver dropped
            }
            height += 1;
        }

        head = store.wait_new_head().await;
    }
}
