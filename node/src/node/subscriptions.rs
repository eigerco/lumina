//! Types and utilities related to header/blob/share subscriptions

use std::sync::Arc;
use std::time::Duration;

use celestia_types::blob::BlobsAtHeight;
use celestia_types::nmt::Namespace;
use celestia_types::row_namespace_data::NamespaceData;
use celestia_types::{Blob, ExtendedHeader, SharesAtHeight};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::NodeError;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, Wrapper};

const SHWAP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Error thrown while processing the subscription
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Error retrieving subscription item
    #[error("Unable to receive subscription item at {height}: {source}")]
    Node {
        /// Height of the subscription item
        height: u64,
        /// Error that occurred
        #[source]
        source: NodeError,
    },
    /// Receiver lagged too far behind and the subscription will restart from the current head
    #[error("Receiver lagged too far behind, skipping {0} items")]
    Lagged(u64),
}

fn reconstruct_blobs(
    namespace_data: NamespaceData,
    header: &ExtendedHeader,
) -> Result<BlobsAtHeight, P2pError> {
    let shares = namespace_data.rows.iter().flat_map(|row| row.shares.iter());
    let blobs = Blob::reconstruct_all(shares, header.app_version()?)?;
    Ok(BlobsAtHeight {
        height: header.height().into(),
        blobs,
    })
}

pub(crate) async fn forward_new_blobs<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<BlobsAtHeight, SubscriptionError>>,
    store: Arc<Wrapper<S>>,
    p2p: Arc<P2p>,
) {
    let mut header_receiver = store.subscribe_headers();

    loop {
        let header = match header_receiver.recv().await {
            Ok(header) => header,
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(Err(SubscriptionError::Lagged(skipped))).await;
                continue;
            }
            Err(RecvError::Closed) => {
                return;
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
            .and_then(|namespace_data| reconstruct_blobs(namespace_data, &header))
            .map_err(|e| SubscriptionError::Node {
                height: header.height().into(),
                source: e.into(),
            });

        if tx.send(blobs_or_error).await.is_err() {
            return; // receiver dropped
        }
    }
}

pub(crate) async fn forward_new_shares<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<SharesAtHeight, SubscriptionError>>,
    store: Arc<Wrapper<S>>,
    p2p: Arc<P2p>,
) {
    let mut header_receiver = store.subscribe_headers();

    loop {
        let header = match header_receiver.recv().await {
            Ok(header) => header,
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(Err(SubscriptionError::Lagged(skipped))).await;
                continue;
            }
            Err(RecvError::Closed) => {
                return;
            }
        };

        let shares_or_error = match p2p
            .get_namespace_data(
                namespace,
                &header,
                Some(SHWAP_FETCH_TIMEOUT),
                store.as_ref(),
            )
            .await
        {
            Ok(namespace_data) => Ok(SharesAtHeight {
                height: header.height().into(),
                shares: namespace_data
                    .rows
                    .into_iter()
                    .flat_map(|row| row.shares.into_iter())
                    .collect(),
            }),
            Err(e) => Err(SubscriptionError::Node {
                height: header.height().into(),
                source: e.into(),
            }),
        };

        if tx.send(shares_or_error).await.is_err() {
            return; // receiver dropped
        }
    }
}

impl From<BroadcastStreamRecvError> for SubscriptionError {
    fn from(BroadcastStreamRecvError::Lagged(skipped): BroadcastStreamRecvError) -> Self {
        SubscriptionError::Lagged(skipped)
    }
}
