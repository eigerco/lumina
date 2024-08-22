use std::sync::Arc;
use std::time::Duration;

use blockstore::Blockstore;
use celestia_tendermint::Time;
use celestia_types::ExtendedHeader;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::events::{EventPublisher, NodeEvent};
use crate::executor::{sleep, spawn};
use crate::p2p::P2pError;
use crate::store::{Store, StoreError};
use crate::syncer::SYNCING_WINDOW;

// pruning window is 1 hour behind syncing window
const PRUNING_WINDOW: Duration = SYNCING_WINDOW.saturating_add(Duration::from_secs(60 * 60));
pub const DEFAULT_PRUNING_INTERVAL: Duration = Duration::from_secs(60);

type Result<T, E = PrunerError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with the [`Pruner`].
#[derive(Debug, thiserror::Error)]
pub enum PrunerError {
    /// An error propagated from the [`P2p`] module.
    #[error(transparent)]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Store`] module.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// An error propagated from the [`Blockstore`] module.
    #[error(transparent)]
    Blockstore(#[from] blockstore::Error),

    /// Pruner removed CIDs for the last header in the store, but the last header changed
    /// before it could be removed (probably becasue of an insert of an older header).
    /// Since pruning window is 1h behind syncing window, this should not happen.
    #[error("Pruner detected invalid removal and will stop")]
    WrongHeightRemoved,
}

pub struct Pruner {
    cancellation_token: CancellationToken,
}

pub struct PrunerArgs<S, B>
where
    S: Store,
    B: Blockstore,
{
    /// Headers storage.
    pub store: Arc<S>,
    /// Block storage.
    pub blockstore: Arc<B>,
    /// Event publisher.
    pub event_pub: EventPublisher,
    /// interval at which pruner will run
    pub pruning_interval: Duration,
}

impl Pruner {
    pub fn start<S, B>(args: PrunerArgs<S, B>) -> Self
    where
        S: Store + 'static,
        B: Blockstore + 'static,
    {
        let cancellation_token = CancellationToken::new();
        let event_pub = args.event_pub.clone();

        let mut worker = Worker::new(args, cancellation_token.child_token());

        spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Pruner stopped because of a fatal error: {e}");

                event_pub.send(NodeEvent::FatalPrunerError {
                    error: e.to_string(),
                });
            }
        });

        Pruner { cancellation_token }
    }

    pub fn stop(&self) {
        self.cancellation_token.cancel();
    }
}

impl Drop for Pruner {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

struct Worker<S, B>
where
    S: Store + 'static,
    B: Blockstore + 'static,
{
    cancellation_token: CancellationToken,
    event_pub: EventPublisher,
    store: Arc<S>,
    blockstore: Arc<B>,
    pruning_interval: Duration,
}

impl<S, B> Worker<S, B>
where
    S: Store,
    B: Blockstore,
{
    fn new(args: PrunerArgs<S, B>, cancellation_token: CancellationToken) -> Self {
        Worker {
            cancellation_token,
            event_pub: args.event_pub,
            store: args.store,
            blockstore: args.blockstore,
            pruning_interval: args.pruning_interval,
        }
    }

    async fn run(&mut self) -> Result<()> {
        let mut last_reported = None;
        let mut last_removed = None;
        loop {
            let pruning_window_end = Time::now().checked_sub(PRUNING_WINDOW).unwrap_or_else(|| {
                warn!("underflow when computing pruning window start, defaulting to unix epoch");
                Time::unix_epoch()
            });

            while let Some(header) = self.get_tail_header_to_prune(&pruning_window_end).await? {
                if self.cancellation_token.is_cancelled() {
                    break;
                }
                let height = header.height().value();

                let cids = self
                    .store
                    .get_sampling_metadata(height)
                    .await?
                    .map(|m| m.cids)
                    .unwrap_or_default();
                for cid in cids {
                    self.blockstore.remove(&cid).await?;
                }

                let removed_height = self.store.remove_last().await?;
                if header.height().value() != removed_height {
                    return Err(PrunerError::WrongHeightRemoved);
                }

                last_removed = Some(height);
            }

            if last_reported != last_removed {
                last_reported = last_removed;
                self.event_pub.send(NodeEvent::PrunedHeaders {
                    to_height: last_removed.expect("last removed height to be set"),
                });
            }

            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = sleep(self.pruning_interval) => ()
            }
        }

        debug!("Pruner stopped");
        Ok(())
    }

    /// Get oldest header from the store to be pruned or None if there's nothing to prune
    async fn get_tail_header_to_prune(&self, cutoff: &Time) -> Result<Option<ExtendedHeader>> {
        let Some(current_tail_height) = self.store.get_stored_header_ranges().await?.tail() else {
            // empty store == nothing to prune
            return Ok(None);
        };

        let header = self.store.get_by_height(current_tail_height).await?;

        if &header.time() > cutoff {
            Ok(None)
        } else {
            Ok(Some(header))
        }
    }
}

#[cfg(test)]
mod test {
    use blockstore::block::{Block, CidError};
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use cid::multihash::Multihash;
    use cid::CidGeneric;

    use super::*;
    use crate::blockstore::InMemoryBlockstore;
    use crate::events::{EventChannel, TryRecvError};
    use crate::store::{InMemoryStore, SamplingStatus};
    use crate::test_utils::{
        async_test, gen_filled_store, new_block_ranges, ExtendedHeaderGeneratorExt,
    };

    const TEST_CODEC: u64 = 0x0D;
    const TEST_MH_CODE: u64 = 0x0D;

    #[async_test]
    async fn empty_store() {
        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();

        let pruner = Pruner::start(PrunerArgs {
            store,
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
        });

        sleep(Duration::from_secs(1)).await;

        pruner.stop();

        sleep(Duration::from_secs(1)).await;

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
    }

    #[async_test]
    async fn nothing_to_prune() {
        let events = EventChannel::new();
        let (store, _gen) = gen_filled_store(100).await;
        let store = Arc::new(store);
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();

        let pruner = Pruner::start(PrunerArgs {
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
        });

        sleep(Duration::from_secs(1)).await;

        pruner.stop();

        sleep(Duration::from_secs(1)).await;

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([1..=100])
        );
    }

    #[async_test]
    async fn prune_large_tail_with_cids() {
        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let mut gen = ExtendedHeaderGenerator::new();

        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();

        let first_header_time =
            (Time::now() - (PRUNING_WINDOW + Duration::from_secs(30 * 24 * 60 * 60))).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(1));

        let blocks_with_sampling = (1..=1000)
            .map(|height| {
                let block = TestBlock::from(height);
                let sampling_status = match height % 3 {
                    0 => SamplingStatus::Unknown,
                    1 => SamplingStatus::Accepted,
                    2 => SamplingStatus::Accepted,
                    _ => unreachable!(),
                };
                (height, block, block.cid().unwrap(), sampling_status)
            })
            .collect::<Vec<_>>();

        store.insert(gen.next_many_verified(1_000)).await.unwrap();

        for (height, block, cid, status) in &blocks_with_sampling {
            blockstore.put_keyed(cid, block.data()).await.unwrap();
            store
                .update_sampling_metadata(*height, *status, vec![*cid])
                .await
                .unwrap()
        }

        let pruner = Pruner::start(PrunerArgs {
            store: store.clone(),
            blockstore: blockstore.clone(),
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
        });

        sleep(Duration::from_secs(1)).await;

        let pruner_event = event_subscriber.recv().await.unwrap().event;
        assert!(matches!(
            pruner_event,
            NodeEvent::PrunedHeaders { to_height: 1_000 }
        ));
        assert!(store.get_stored_header_ranges().await.unwrap().is_empty());

        for (height, _, cid, _) in &blocks_with_sampling {
            assert!(matches!(
                store.get_sampling_metadata(*height).await.unwrap_err(),
                StoreError::NotFound
            ));
            assert!(!blockstore.has(cid).await.unwrap());
        }

        pruner.stop();

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
    }

    #[async_test]
    async fn prune_tail() {
        const BLOCK_TIME: Duration = Duration::from_millis(10);

        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let mut gen = ExtendedHeaderGenerator::new();
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();

        // 50 headers before pruning window edge
        let before_pruning_edge = (Time::now() - (PRUNING_WINDOW + BLOCK_TIME * 100)).unwrap();
        gen.set_time(before_pruning_edge, BLOCK_TIME);
        store.insert(gen.next_many_verified(50)).await.unwrap();

        // 10 headers within 1sec of pruning window edge
        let after_pruning_edge = (Time::now() - (PRUNING_WINDOW - BLOCK_TIME * 10)).unwrap();
        gen.set_time(after_pruning_edge, BLOCK_TIME);
        store.insert(gen.next_many_verified(10)).await.unwrap();

        // 50 headers at current time
        gen.set_time(Time::now(), BLOCK_TIME);
        store.insert(gen.next_many_verified(10)).await.unwrap();

        let pruner = Pruner::start(PrunerArgs {
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
        });

        sleep(Duration::from_secs(1)).await;

        let pruner_event = event_subscriber.recv().await.unwrap().event;
        assert!(matches!(
            pruner_event,
            NodeEvent::PrunedHeaders { to_height: 50 }
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([51..=70])
        );

        sleep(Duration::from_secs(2)).await;

        let pruner_event = event_subscriber.try_recv().unwrap().event;
        assert!(matches!(
            pruner_event,
            NodeEvent::PrunedHeaders { to_height: 60 }
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([61..=70])
        );

        pruner.stop();
        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([61..=70])
        );
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    struct TestBlock(pub [u8; 8]);

    impl From<u64> for TestBlock {
        fn from(value: u64) -> Self {
            TestBlock(value.to_le_bytes())
        }
    }

    impl Block<64> for TestBlock {
        fn cid(&self) -> Result<CidGeneric<64>, CidError> {
            let mh = Multihash::wrap(TEST_MH_CODE, &self.0).unwrap();
            Ok(CidGeneric::new_v1(TEST_CODEC, mh))
        }

        fn data(&self) -> &[u8] {
            &self.0
        }
    }
}
