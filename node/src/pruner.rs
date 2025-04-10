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
        let mut removed_range = None;

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

                removed_range = match removed_range.take() {
                    Some((from_height, to_height)) => {
                        // If `height` is a neightbor to previously removed heaaders then
                        // we update the `removed_range`, otherwise we create an event
                        // for the previous headers.
                        if to_height == height - 1 {
                            Some((from_height, height))
                        } else {
                            self.event_pub.send(NodeEvent::PrunedHeaders {
                                from_height,
                                to_height,
                            });
                            Some((height, height))
                        }
                    }
                    None => Some((height, height)),
                }
            }

            if let Some((from_height, to_height)) = removed_range.take() {
                self.event_pub.send(NodeEvent::PrunedHeaders {
                    from_height,
                    to_height,
                });
            }

            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = sleep(self.pruning_interval) => ()
            }
        }

        if let Some((from_height, to_height)) = removed_range.take() {
            self.event_pub.send(NodeEvent::PrunedHeaders {
                from_height,
                to_height,
            });
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
                // If block is outside of sampling window then we need to prune it.
                // If it wasn't sampled yet, then Daser must allow us first. We do
                // this to avoid race conditions and other edge cases between Pruner
                // and Daser.
                sampled_ranges.contains(height) || self.daser.want_to_prune(height).await?
            } else if edges.contains(height) {
                // If block in inside the sampling window and an edge, then we keep it.
                // We need it to verify missing neighbors later on.
                false
            } else if sampled_ranges.contains(height) {
                // If block is inside the sampling window, not an edge, and got sampled,
                // then we prune it.
                true
            } else {
                // If block is inside the sampling window, not an edge, and not sampled,
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
    use std::time::Duration;

    use blockstore::block::{Block, CidError};
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use cid::multihash::Multihash;
    use cid::CidGeneric;

    use super::*;
    use crate::blockstore::InMemoryBlockstore;
    use crate::events::{EventChannel, EventSubscriber, TryRecvError};
    use crate::node::{DEFAULT_PRUNING_WINDOW, DEFAULT_SAMPLING_WINDOW};
    use crate::store::InMemoryStore;
    use crate::test_utils::{gen_filled_store, new_block_ranges, ExtendedHeaderGeneratorExt};
    use lumina_utils::test_utils::async_test;
    use lumina_utils::time::timeout;

    const TEST_CODEC: u64 = 0x0D;
    const TEST_MH_CODE: u64 = 0x0D;

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
            pruning_window: DEFAULT_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        sleep(Duration::from_secs(1)).await;

        daser_handle.expect_no_cmd().await;
        pruner.stop();
        pruner.join().await;

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
            pruning_window: DEFAULT_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        sleep(Duration::from_secs(1)).await;

        daser_handle.expect_no_cmd().await;
        pruner.stop();
        pruner.join().await;

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
        let (daser, mut daser_handle) = Daser::mocked();

        let first_header_time = (Time::now()
            - (DEFAULT_PRUNING_WINDOW + Duration::from_secs(30 * 24 * 60 * 60)))
        .unwrap();
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
                store.mark_as_sampled(*height).await.unwrap();
            }

            store
                .update_sampling_metadata(*height, vec![*cid])
                .await
                .unwrap()
        }

        let sampled_ranges = store.get_sampled_ranges().await.unwrap();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore: blockstore.clone(),
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window: DEFAULT_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        for height in 1..=1000 {
            // If a block is not sampled, Pruner asks Daser for permission to prune it.
            if !sampled_ranges.contains(height) {
                let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
                assert_eq!(want_to_prune, height);
                respond_to.send(true).unwrap();
            }
        }

        assert_pruned_headers_event(&mut event_subscriber, 1, 1000).await;

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
        pruner.join().await;

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
        let (daser, mut daser_handle) = Daser::mocked();

        // 50 headers before pruning window edge
        let before_pruning_edge =
            (Time::now() - (DEFAULT_PRUNING_WINDOW + BLOCK_TIME * 100)).unwrap();
        gen.set_time(before_pruning_edge, BLOCK_TIME);
        store.insert(gen.next_many_verified(50)).await.unwrap();

        // 10 headers within 1sec of pruning window edge
        let after_pruning_edge =
            (Time::now() - (DEFAULT_PRUNING_WINDOW - BLOCK_TIME * 10)).unwrap();
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
            pruning_window: DEFAULT_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        daser_handle.handle_want_to_prune(1..=50).await;

        assert_pruned_headers_event(&mut event_subscriber, 1, 50).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([51..=70])
        );

        sleep(Duration::from_secs(1)).await;

        daser_handle.handle_want_to_prune(51..=60).await;

        assert_pruned_headers_event(&mut event_subscriber, 51, 60).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([61..=70])
        );

        daser_handle.expect_no_cmd().await;
        pruner.stop();
        pruner.join().await;

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([61..=70])
        );
    }

    #[async_test]
    async fn sampling_window_bigger_than_pruning_window() {
        let pruning_window = Duration::from_secs(60);
        let sampling_window = Duration::from_secs(120);

        let mut gen = ExtendedHeaderGenerator::new();
        let store = Arc::new(InMemoryStore::new());
        let blockstore = Arc::new(InMemoryBlockstore::new());

        // Start header creation 260 seconds in the past, with block time of 1 second.
        // Since pruning window is 60s and sampling window is 120s, this means:
        //
        // +---------------------------------------------------------+
        // |       1 - 140       |   141 - 200   |      201 - 260    |
        // +---------------------+---------------+-------------------+
        // <----------Prunable area------------->|<--Pruning window-->
        //                       <-----------Sampling window--------->
        //
        // NOTE 1: Pruning window is actually "keep in store" window. Even if its name
        // can be confusing, we kept it because this is how it meantioned in CIP-4.
        //
        // NOTE 2: Since `Time::now` in Pruner is changing, the edges may vary.
        let first_header_time = (Time::now() - Duration::from_secs(260)).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(1));

        // 1 - 120
        store.insert(gen.next_many_verified(120)).await.unwrap();
        // 121 - 145
        gen.skip(25);
        // 146 - 155
        store.insert(gen.next_many_verified(10)).await.unwrap();
        // 156 - 165
        gen.skip(10);
        // 166 - 175
        store.insert(gen.next_many_verified(10)).await.unwrap();
        // 176 - 185
        gen.skip(10);
        // 186 - 199
        store.insert(gen.next_many_verified(14)).await.unwrap();
        // 200 - 201, We skip these because they are the edge of pruning window.
        // Pruner is using `Time::now` and we want to make the tests more predictable.
        gen.skip(2);
        // 202 - 260
        store.insert(gen.next_many_verified(59)).await.unwrap();

        // Create gaps by pruning. We do this to test that Pruner takes them
        // into account when it calculates the edges of synced block ranges.
        store.remove_height(188).await.unwrap();
        store.remove_height(189).await.unwrap();
        store.remove_height(192).await.unwrap();

        for height in (1..=100).chain(103..=195).chain(199..=210) {
            // We ignore the error because we want to get makred only
            // those that are in store.
            let _ = store.mark_as_sampled(height).await;
        }

        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                1..=120,
                146..=155,
                166..=175,
                186..=187,
                190..=191,
                193..=199,
                202..=260
            ])
        );
        assert_eq!(
            store.get_sampled_ranges().await.unwrap(),
            new_block_ranges([
                1..=100,
                103..=120,
                146..=155,
                166..=175,
                186..=187,
                190..=191,
                193..=195,
                199..=199,
                202..=210
            ])
        );
        assert_eq!(
            store.get_pruned_ranges().await.unwrap(),
            new_block_ranges([188..=189, 192..=192])
        );

        let events = EventChannel::new();
        let mut event_subscriber = events.subscribe();
        let (daser, mut daser_handle) = Daser::mocked();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            pruning_interval: Duration::from_secs(1),
            pruning_window,
            sampling_window,
        });

        // Pruner removed all headers until 100 (included), because they were sampled.
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                101..=120,
                146..=155,
                166..=175,
                186..=187,
                190..=191,
                193..=199,
                202..=260
            ])
        );
        assert_eq!(
            store.get_pruned_ranges().await.unwrap(),
            new_block_ranges([1..=100, 188..=189, 192..=192])
        );

        // Pruner asks for Daser if it can remove block 101.
        // We simulate that Daser does not allow it.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 101);
        respond_to.send(false).unwrap();

        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                101..=120,
                146..=155,
                166..=175,
                186..=187,
                190..=191,
                193..=199,
                202..=260
            ])
        );
        assert_eq!(
            store.get_pruned_ranges().await.unwrap(),
            new_block_ranges([1..=100, 188..=189, 192..=192])
        );

        // Because Daser didn't allow it, Pruner will ask for the next one.
        // We simulate that Daser allows pruning of 102.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 102);
        respond_to.send(true).unwrap();

        // Pruner generates the event for 1-100 pruned range because
        // 102 is not continuous to it. We will receive the 102 event
        // after the next prune.
        assert_pruned_headers_event(&mut event_subscriber, 1, 100).await;

        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                101..=101,
                103..=120,
                146..=155,
                166..=175,
                186..=187,
                190..=191,
                193..=199,
                202..=260
            ])
        );
        assert_eq!(
            store.get_pruned_ranges().await.unwrap(),
            new_block_ranges([1..=100, 102..=102, 188..=189, 192..=192])
        );

        // Now Pruner will ask again for 101, but this time we allow it.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 101);
        respond_to.send(true).unwrap();

        assert_pruned_headers_event(&mut event_subscriber, 102, 102).await;
        assert_pruned_headers_event(&mut event_subscriber, 101, 101).await;

        // Because Pruner runs in parallel with this test and we don't
        // expect any other commands towards Daser, we need to check
        // the final state.
        //
        // We expect Pruner to do the following:
        //
        // - Consider blocks 188, 189, 192 as synced because they exists in pruned ranges.
        // - Remove 103-120, 147-154, 167-174, 187, 190-191, 193-195.
        // - Keep 146, 155, 166, 175, 186 blocks because they are
        //   edges (i.e. the blocks that are neightbors of non-synced ranges).
        // - Keep 196-199 because they are in sampling window and not sampled yet.
        // - Keep 202-260 because they are in pruning window.
        assert_pruned_headers_event(&mut event_subscriber, 103, 120).await;
        assert_pruned_headers_event(&mut event_subscriber, 147, 154).await;
        assert_pruned_headers_event(&mut event_subscriber, 167, 174).await;
        assert_pruned_headers_event(&mut event_subscriber, 187, 187).await;
        assert_pruned_headers_event(&mut event_subscriber, 190, 191).await;
        assert_pruned_headers_event(&mut event_subscriber, 193, 195).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                146..=146,
                155..=155,
                166..=166,
                175..=175,
                186..=186,
                196..=199,
                202..=260
            ])
        );
        assert_eq!(
            store.get_pruned_ranges().await.unwrap(),
            new_block_ranges([1..=120, 147..=154, 167..=174, 187..=195])
        );

        daser_handle.expect_no_cmd().await;
        pruner.stop();
        pruner.join().await;

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
    }

    async fn assert_pruned_headers_event(
        subscriber: &mut EventSubscriber,
        from_height: u64,
        to_height: u64,
    ) {
        let event = timeout(Duration::from_secs(1), subscriber.recv())
            .await
            .unwrap_or_else(|_| {
                panic!("Expecting PrunedHeaders({from_height}-{to_height}) event but nothing is received")
            })
            .unwrap()
            .event;

        match event {
            NodeEvent::PrunedHeaders {
                from_height: from,
                to_height: to,
            } => {
                assert_eq!((from, to), (from_height, to_height));
            }
            ev => panic!(
                "Expecting PrunedHeaders({from_height}-{to_height}) event, but received: {ev:?}"
            ),
        }
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
