use std::sync::Arc;
use std::time::Duration;

use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespaceData;
use celestia_types::{Blob, ExtendedHeader};
use tokio::sync::mpsc;

use crate::NodeError;
use crate::p2p::P2p;
use crate::store::Store;

const SHWAP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Error thrown while processing subscription
#[derive(Debug, thiserror::Error)]
#[error("Unable to receive subscription item at {height}: {source}")]
pub struct SubscriptionError {
    /// Height of the subscription item
    pub height: u64,
    /// Error that occured
    #[source]
    pub source: NodeError,
}

pub(crate) async fn forward_new_headers<S: Store>(
    store: Arc<S>,
    tx: mpsc::Sender<Result<ExtendedHeader, SubscriptionError>>,
) {
    let mut prev_head = store.wait_new_head().await;
    let mut new_head = prev_head;

    loop {
        for height in prev_head..=new_head {
            let header_or_error =
                store
                    .get_by_height(height)
                    .await
                    .map_err(|e| SubscriptionError {
                        height,
                        source: NodeError::Store(e),
                    });
            if tx.send(header_or_error).await.is_err() {
                return; // receiver dropped
            }
        }
        prev_head = new_head + 1;
        new_head = store.wait_new_head().await;
    }
}

pub(crate) async fn forward_new_blobs<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<(u64, Vec<Blob>), SubscriptionError>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut prev_head = store.wait_new_head().await;
    let mut new_head = prev_head;

    loop {
        for height in prev_head..=new_head {
            let blobs_or_error = p2p
                .get_all_blobs(namespace, height, Some(SHWAP_FETCH_TIMEOUT), store.as_ref())
                .await
                .map(|blobs| (height, blobs))
                .map_err(|e| SubscriptionError {
                    height,
                    source: NodeError::P2p(e),
                });
            if tx.send(blobs_or_error).await.is_err() {
                return; // receiver dropped
            }
        }
        prev_head = new_head + 1;
        new_head = store.wait_new_head().await;
    }
}

pub(crate) async fn forward_new_shares<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<(u64, NamespaceData), SubscriptionError>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut prev_head = store.wait_new_head().await;
    let mut new_head = prev_head;

    loop {
        for height in prev_head..=new_head {
            let header = match store.get_by_height(height).await {
                Ok(h) => h,
                Err(e) => {
                    if tx
                        .send(Err(SubscriptionError {
                            height,
                            source: NodeError::Store(e),
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
            };
            let blobs_or_error = p2p
                .get_namespace_data(
                    namespace,
                    &header,
                    Some(SHWAP_FETCH_TIMEOUT),
                    store.as_ref(),
                )
                .await
                .map(|blobs| (height, blobs))
                .map_err(|e| SubscriptionError {
                    height,
                    source: NodeError::P2p(e),
                });
            if tx.send(blobs_or_error).await.is_err() {
                return; // receiver dropped
            }
        }
        prev_head = new_head + 1;
        new_head = store.wait_new_head().await;
    }
}
