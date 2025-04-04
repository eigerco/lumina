use std::sync::Arc;
use std::time::Duration;

use blockstore::Blockstore;
use celestia_types::ExtendedHeader;
use lumina_utils::executor::{spawn, JoinHandle};
use lumina_utils::time::sleep;
use tendermint::Time;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::daser::{Daser, DaserError};
use crate::events::{EventPublisher, NodeEvent};
use crate::p2p::P2pError;
use crate::store::{Store, StoreError};

pub const DEFAULT_PRUNING_INTERVAL: Duration = Duration::from_secs(12);

type Result<T, E = PrunerError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with the [`Pruner`].
#[derive(Debug, thiserror::Error)]
pub(crate) enum PrunerError {
    /// An error propagated from the [`P2p`] module.
    #[error("P2p: {0}")]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Store`] module.
    #[error("Syncer: {0}")]
    Store(#[from] StoreError),

    /// An error propagated from the [`Blockstore`] module.
    #[error("Blockstore: {0}")]
    Blockstore(#[from] blockstore::Error),

    /// An error propagated from the `Daser` component.
    #[error("Daser: {0}")]
    Daser(#[from] DaserError),
}

pub(crate) struct Pruner {
    cancellation_token: CancellationToken,
    join_handle: JoinHandle,
}

pub(crate) struct PrunerArgs<S, B>
where
    S: Store,
    B: Blockstore,
{
    pub daser: Arc<Daser>,
    /// Headers storage.
    pub store: Arc<S>,
    /// Block storage.
    pub blockstore: Arc<B>,
    /// Event publisher.
    pub event_pub: EventPublisher,
    /// interval at which pruner will run
    pub pruning_interval: Duration,
    /// Size of pruning window
    pub pruning_window: Duration,
    /// Size of sampling window
    pub sampling_window: Duration,
}

impl Pruner {
    pub(crate) fn start<S, B>(args: PrunerArgs<S, B>) -> Self
    where
        S: Store + 'static,
        B: Blockstore + 'static,
    {
        let cancellation_token = CancellationToken::new();
        let event_pub = args.event_pub.clone();

        let mut worker = Worker::new(args, cancellation_token.child_token());

        let join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Pruner stopped because of a fatal error: {e}");

                event_pub.send(NodeEvent::FatalPrunerError {
                    error: e.to_string(),
                });
            }
        });

        Pruner {
            cancellation_token,
            join_handle,
        }
    }

    /// Stop the worker.
    pub(crate) fn stop(&self) {
        // Singal the Worker to stop.
        self.cancellation_token.cancel();
    }

    /// Wait until worker is completely stopped.
    pub(crate) async fn join(&self) {
        self.join_handle.join().await;
    }
}

impl Drop for Pruner {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Worker<S, B>
where
    S: Store + 'static,
    B: Blockstore + 'static,
{
    daser: Arc<Daser>,
    cancellation_token: CancellationToken,
    event_pub: EventPublisher,
    store: Arc<S>,
    blockstore: Arc<B>,
    pruning_interval: Duration,
    pruning_window: Duration,
    sampling_window: Duration,
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
            pruning_window: args.pruning_window,
            sampling_window: args.sampling_window,
            daser: args.daser,
        }
    }

    async fn run(&mut self) -> Result<()> {
        let mut last_removed = None;
        let mut last_reported = None;

        loop {
            let now = Time::now();

            let sampling_window_end = now.checked_sub(self.sampling_window).unwrap_or_else(|| {
                warn!("underflow when computing sampling window, defaulting to unix epoch");
                Time::unix_epoch()
            });

            let pruning_window_end = now.checked_sub(self.pruning_window).unwrap_or_else(|| {
                warn!("underflow when computing pruning window, defaulting to unix epoch");
                Time::unix_epoch()
            });

            while let Some(header) = self
                .get_next_header_to_prune(sampling_window_end, pruning_window_end)
                .await?
            {
                if self.cancellation_token.is_cancelled() {
                    break;
                }

                let height = header.height().value();

                let sampling_metadata = self
                    .store
                    .get_sampling_metadata(height)
                    .await?
                    .unwrap_or_default();

                for cid in sampling_metadata.cids {
                    self.blockstore.remove(&cid).await?;
                }

                self.store.remove_height(height).await?;
                last_removed = Some(height);
            }

            if last_reported != last_removed {
                last_reported = last_removed;

                self.event_pub.send(NodeEvent::PrunedHeaders {
                    // TODO: This does not represent the current pruning algorithm
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

    /// Get next header from the store to be pruned or None if there's nothing to prune
    async fn get_next_header_to_prune(
        &self,
        sampling_cutoff: Time,
        pruning_cutoff: Time,
    ) -> Result<Option<ExtendedHeader>> {
        let stored_ranges = self.store.get_stored_header_ranges().await?;
        let pruned_ranges = self.store.get_pruned_ranges().await?;
        let sampled_ranges = self.store.get_sampled_ranges().await?;

        // All synced heights (stored + pruned)
        let synced_ranges = pruned_ranges + &stored_ranges;
        // Edges of the synced ranges
        let edges = synced_ranges.edges();

        // Iterate from the oldest height to the newest.
        for height in stored_ranges {
            let header = self.store.get_by_height(height).await?;

            // Heights in `stored_ranges` are ordered, so if a height didn't
            // reach the pruning cutoff, so are the next ones.
            // In other words, we have nothing to prune for now.
            if header.time() > pruning_cutoff {
                return Ok(None);
            }

            let needs_pruning = if header.time() <= sampling_cutoff {
                // If height is outside of sampling window then we need to prune it.
                // However Daser must allow us first. We do this to avoid race conditions
                // and other edge cases between Pruner and Daser.
                self.daser.want_to_prune(height).await?
            } else if edges.contains(height) {
                // If height in inside the sampling window and an edge, then we keep it.
                // We need it to verify missing neighbors later on.
                false
            } else if sampled_ranges.contains(height) {
                // If height is inside the sampling window, not an edge, and got sampled,
                // then we prune it.
                true
            } else {
                // If height is inside the sampling window, not an edge, and not sampled,
                // then we keep it.
                false
            };

            // All constrains are met and we are allowed to prune this block.
            if needs_pruning {
                return Ok(Some(header));
            }
        }

        Ok(None)
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
    use crate::node::{DEFAULT_PRUNING_WINDOW, DEFAULT_SAMPLING_WINDOW};
    use crate::store::{InMemoryStore};
    use crate::test_utils::{gen_filled_store, new_block_ranges, ExtendedHeaderGeneratorExt};
    use lumina_utils::test_utils::async_test;

    const TEST_CODEC: u64 = 0x0D;
    const TEST_MH_CODE: u64 = 0x0D;
    const TEST_PRUNING_WINDOW: Duration = DEFAULT_PRUNING_WINDOW;

    #[async_test]
    async fn empty_store() {
        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();
        let (daser, mut daser_handle) = Daser::mocked();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store,
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window: TEST_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        sleep(Duration::from_secs(1)).await;

        daser_handle.expect_no_cmd().await;
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
        let (daser, mut daser_handle) = Daser::mocked();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window: TEST_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        sleep(Duration::from_secs(1)).await;

        daser_handle.expect_no_cmd().await;
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
    async fn prune_large_tail_with_cids_and_sampling_smaller_than_pruning_window() {
        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let mut gen = ExtendedHeaderGenerator::new();

        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();
        let (daser, mut daser_handle) = Daser::mocked();

        let first_header_time =
            (Time::now() - (TEST_PRUNING_WINDOW + Duration::from_secs(30 * 24 * 60 * 60))).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(1));

        let blocks_with_sampling = (1..=1000)
            .map(|height| {
                let block = TestBlock::from(height);
                let sampled = match height % 3 {
                    0 => false,
                    _ => true,
                };
                (height, block, block.cid().unwrap(), sampled)
            })
            .collect::<Vec<_>>();

        store.insert(gen.next_many_verified(1_000)).await.unwrap();

        for (height, block, cid, sampled) in &blocks_with_sampling {
            blockstore.put_keyed(cid, block.data()).await.unwrap();

            if *sampled {
                store.mark_sampled(*height).await.unwrap();
            }

            store
                .update_sampling_metadata(*height, vec![*cid])
                .await
                .unwrap()
        }

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore: blockstore.clone(),
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window: TEST_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        daser_handle.handle_want_to_prune(1..=1000).await;

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

        daser_handle.expect_no_cmd().await;
        pruner.stop();

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
    }

    #[async_test]
    async fn prune_tail_sampling_smaller_than_pruning_window() {
        const BLOCK_TIME: Duration = Duration::from_millis(10);

        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let mut gen = ExtendedHeaderGenerator::new();
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();
        let (daser, mut daser_handle) = Daser::mocked();

        // 50 headers before pruning window edge
        let before_pruning_edge = (Time::now() - (TEST_PRUNING_WINDOW + BLOCK_TIME * 100)).unwrap();
        gen.set_time(before_pruning_edge, BLOCK_TIME);
        store.insert(gen.next_many_verified(50)).await.unwrap();

        // 10 headers within 1sec of pruning window edge
        let after_pruning_edge = (Time::now() - (TEST_PRUNING_WINDOW - BLOCK_TIME * 10)).unwrap();
        gen.set_time(after_pruning_edge, BLOCK_TIME);
        store.insert(gen.next_many_verified(10)).await.unwrap();

        // 50 headers at current time
        gen.set_time(Time::now(), BLOCK_TIME);
        store.insert(gen.next_many_verified(10)).await.unwrap();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window: TEST_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        daser_handle.handle_want_to_prune(1..=50).await;

        let pruner_event = event_subscriber.recv().await.unwrap().event;
        assert!(matches!(
            pruner_event,
            NodeEvent::PrunedHeaders { to_height: 50 }
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([51..=70])
        );

        sleep(Duration::from_secs(1)).await;

        daser_handle.handle_want_to_prune(51..=60).await;

        let pruner_event = event_subscriber.recv().await.unwrap().event;
        assert!(matches!(
            pruner_event,
            NodeEvent::PrunedHeaders { to_height: 60 }
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([61..=70])
        );

        daser_handle.expect_no_cmd().await;
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
