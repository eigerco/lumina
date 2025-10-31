//! Types and utilities related to header/blob/share subscriptions

use std::sync::Arc;
use std::time::Duration;

use celestia_types::blob::BlobsAtHeight;
use celestia_types::nmt::Namespace;
use celestia_types::{ExtendedHeader, SharesAtHeight};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc};
use tracing::error;

use crate::NodeError;
use crate::p2p::P2p;
use crate::store::{BlockRange, BlockRanges, Store};
use crate::syncer::Syncer;

const SHWAP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const HEIGHT_SEQUENCER_BROADCAST_CAPACITY: usize = 16;

/// Error thrown while processing the subscription
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Error retrieving subscription item
    #[error("Unable to receive subscription item at {height}: {source}")]
    Node {
        /// Height of the subscription item
        height: u64,
        /// Error that occured
        #[source]
        source: NodeError,
    },
    /// Receiver lagged too far behind and the subscription will restart from the current head
    #[error("Subscription item height already pruned from the store, skipping {0} items")]
    Lagged(u64),
}

/// As it gets notified about new header ranges being inserted, it generates a contiguous
/// stream of header heights present in store.
#[derive(Debug)]
pub(crate) struct HeightSequencer {
    sender: broadcast::Sender<u64>,
    last_sent_height: Option<u64>,
    pending: BlockRanges,
}

impl HeightSequencer {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(HEIGHT_SEQUENCER_BROADCAST_CAPACITY);
        HeightSequencer {
            sender,
            last_sent_height: None,
            pending: Default::default(),
        }
    }

    pub(crate) fn init(&mut self, height: u64) {
        if self.last_sent_height.is_none() {
            // first initialisation means syncer has acquired a network head for the first time
            // lets start from there
            self.last_sent_height = Some(height);
        } else {
            // subsequent initialisations happen when syncer re-connects to the network
            // this could have caused a gap in sent heights. This will get sorted out on
            // next [`signal_inserted`].
            self.pending
                .insert_relaxed(height..=height)
                .expect("single item range should be valid");
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<u64> {
        self.sender.subscribe()
    }

    pub(crate) fn signal_inserted(&mut self, range: BlockRange) {
        let Some(last_sent_height) = self.last_sent_height else {
            // header insertions before head height is initialised
            // this shouldn't happen, ignore it
            return;
        };

        if &last_sent_height > range.end() {
            // we don't care about past ranges syncer is fetching
            return;
        }

        self.pending
            .insert_relaxed(&range)
            .expect("inserted block range should be valid");

        let Some(first_pending) = self.pending.tail() else {
            return;
        };

        if last_sent_height + 1 == first_pending {
            let to_send = self
                .pending
                .pop_tail_range()
                .expect("pending shouldn't be empty");
            self.last_sent_height = Some(*to_send.end());
            for h in to_send {
                let _ = self.sender.send(h);
            }
        }
    }
}

pub(crate) async fn forward_new_headers<S: Store>(
    syncer: Arc<Syncer<S>>,
    store: Arc<S>,
    tx: mpsc::Sender<Result<ExtendedHeader, SubscriptionError>>,
) {
    let mut heights_receiver = syncer.subscribe_heights().await.unwrap();

    loop {
        let height = match heights_receiver.recv().await {
            Ok(height) => height,
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(Err(SubscriptionError::Lagged(skipped))).await;
                continue;
            }
            Err(RecvError::Closed) => {
                error!("unexpected heigth stream close");
                return;
            }
        };

        let header_or_error = match store.get_by_height(height).await {
            Ok(h) => Ok(h),
            Err(e) => Err(SubscriptionError::Node {
                height,
                source: NodeError::Store(e),
            }),
        };

        if tx.send(header_or_error).await.is_err() {
            return; // receiver dropped
        }
    }
}

pub(crate) async fn forward_new_blobs<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<BlobsAtHeight, SubscriptionError>>,
    syncer: Arc<Syncer<S>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut heights_receiver = syncer.subscribe_heights().await.unwrap();

    loop {
        let height = match heights_receiver.recv().await {
            Ok(height) => height,
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(Err(SubscriptionError::Lagged(skipped))).await;
                continue;
            }
            Err(RecvError::Closed) => {
                error!("unexpected heigth stream close");
                return;
            }
        };

        let blobs_or_error = match p2p
            .get_all_blobs(namespace, height, Some(SHWAP_FETCH_TIMEOUT), store.as_ref())
            .await
        {
            Ok(blobs) => Ok(BlobsAtHeight { height, blobs }),
            Err(e) => Err(SubscriptionError::Node {
                height,
                source: e.into(),
            }),
        };

        if tx.send(blobs_or_error).await.is_err() {
            return; // receiver dropped
        }
    }
}

pub(crate) async fn forward_new_shares<S: Store>(
    namespace: Namespace,
    tx: mpsc::Sender<Result<SharesAtHeight, SubscriptionError>>,
    syncer: Arc<Syncer<S>>,
    store: Arc<S>,
    p2p: Arc<P2p>,
) {
    let mut heights_receiver = syncer.subscribe_heights().await.unwrap();

    loop {
        let height = match heights_receiver.recv().await {
            Ok(height) => height,
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(Err(SubscriptionError::Lagged(skipped))).await;
                continue;
            }
            Err(RecvError::Closed) => {
                error!("unexpected heigth stream close");
                return;
            }
        };

        let header = match store.get_by_height(height).await {
            Ok(h) => h,
            Err(e) => {
                if tx
                    .send(Err(SubscriptionError::Node {
                        height,
                        source: e.into(),
                    }))
                    .await
                    .is_ok()
                {
                    continue;
                } else {
                    return;
                }
            }
        };

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumina_utils::executor::{spawn, yield_now};
    use lumina_utils::test_utils::async_test;

    #[async_test]
    async fn test_height_sequencer() {
        let mut seq = HeightSequencer::new();
        seq.init(1);
        let mut hs = seq.subscribe();

        spawn(async move {
            seq.signal_inserted(2..=2);
            yield_now().await;
            seq.signal_inserted(4..=4);
            yield_now().await;
            seq.signal_inserted(3..=3);
            yield_now().await;
            seq.signal_inserted(9..=9);
            yield_now().await;
            seq.signal_inserted(5..=8);
        });

        for i in 2..=9 {
            assert_eq!(i, hs.recv().await.unwrap());
        }
    }
}
