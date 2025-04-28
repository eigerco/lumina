//! Component responsible for removing blocks that aren't needed anymore.

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
use crate::store::{BlockRanges, Store, StoreError};

const MAX_PRUNABLE_BATCH_SIZE: u64 = 512;

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
    /// After how much time a new block is produced.
    pub block_time: Duration,
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
    block_time: Duration,
    pruning_window: Duration,
    sampling_window: Duration,
    cached_head: Option<ExtendedHeader>,
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
            block_time: args.block_time,
            pruning_window: args.pruning_window,
            sampling_window: args.sampling_window,
            daser: args.daser,
            cached_head: None,
        }
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            if self.cancellation_token.is_cancelled() {
                break;
            }

            let now = Time::now();

            let sampling_window_end = now.checked_sub(self.sampling_window).unwrap_or_else(|| {
                warn!("underflow when computing sampling window, defaulting to unix epoch");
                Time::unix_epoch()
            });

            let pruning_window_end = now.checked_sub(self.pruning_window).unwrap_or_else(|| {
                warn!("underflow when computing pruning window, defaulting to unix epoch");
                Time::unix_epoch()
            });

            let prunable_batch = self
                .get_next_prunable_batch(sampling_window_end, pruning_window_end)
                .await?;

            if prunable_batch.is_empty() {
                select! {
                    _ = self.cancellation_token.cancelled() => break,
                    _ = sleep(self.block_time) => continue,
                }
            }

            for range in prunable_batch.into_inner() {
                let from_height = *range.start();
                let mut to_height = None;

                for height in range {
                    if self.cancellation_token.is_cancelled() {
                        break;
                    }

                    let cids = self
                        .store
                        .get_sampling_metadata(height)
                        .await?
                        .map(|m| m.cids)
                        .unwrap_or_default();

                    for cid in cids {
                        self.blockstore.remove(&cid).await?;
                    }

                    self.store.remove_height(height).await?;
                    to_height = Some(height);
                }

                if let Some(to_height) = to_height {
                    self.event_pub.send(NodeEvent::PrunedHeaders {
                        from_height,
                        to_height,
                    });
                }
            }
        }

        debug!("Pruner stopped");
        Ok(())
    }

    /// Get next header from the store to be pruned or None if there's nothing to prune
    async fn get_next_prunable_batch(
        &mut self,
        sampling_cutoff: Time,
        pruning_cutoff: Time,
    ) -> Result<BlockRanges> {
        let mut stored_ranges = self.store.get_stored_header_ranges().await?;

        let Some(stored_head_height) = stored_ranges.head() else {
            return Ok(BlockRanges::new());
        };

        if self
            .cached_head
            .as_ref()
            .is_none_or(|header| header.height().value() != stored_head_height)
        {
            self.cached_head = Some(self.store.get_by_height(stored_head_height).await?);
        }

        let head = self.cached_head.as_ref().unwrap();

        let Some(estimated_prunable_height) =
            estimate_highest_prunable_height(head, pruning_cutoff, self.block_time)
        else {
            return Ok(BlockRanges::new());
        };

        // We do not need to check any blocks that are higher than the
        // estimated prunable height.
        stored_ranges
            .remove_relaxed(estimated_prunable_height + 1..=u64::MAX)
            .expect("never fails");

        let pruned_ranges = self.store.get_pruned_ranges().await?;
        let sampled_ranges = self.store.get_sampled_ranges().await?;

        // All synced heights (stored + pruned)
        let synced_ranges = pruned_ranges + &stored_ranges;
        // Edges of the synced ranges
        let edges = synced_ranges.edges();

        let mut prunable_batch = BlockRanges::new();

        // Iterate from the newest height to the oldest.
        for height in stored_ranges.rev() {
            if self.cancellation_token.is_cancelled() {
                break;
            }

            let header = self.store.get_by_height(height).await?;

            let needs_pruning = if header.time() > pruning_cutoff {
                // If block is inside pruning window, then we keep it.
                false
            } else if header.time() <= sampling_cutoff {
                // If block is outside of pruning window and sampling window then
                // we need to prune it. If it wasn't sampled yet, then Daser must
                // allow us first. We do this to avoid race conditions and other
                // edge cases between Pruner and Daser.
                sampled_ranges.contains(height) || self.daser.want_to_prune(height).await?
            } else if edges.contains(height) {
                // If block is outside of pruning window, inside the sampling window,
                // and an edge, then we keep it. We need it to verify missing neighbors later on.
                false
            } else if sampled_ranges.contains(height) {
                // If block is outside of pruning window, inside the sampling window,
                // not an edge, and got sampled, then we prune it.
                true
            } else {
                // If block is outside the pruning_window, inside the sampling window,
                // not an edge, and not sampled, then we keep it.
                false
            };

            // All constrains are met and we are allowed to prune this block.
            if needs_pruning {
                prunable_batch
                    .insert_relaxed(height..=height)
                    .expect("never fails");

                if prunable_batch.len() >= MAX_PRUNABLE_BATCH_SIZE {
                    break;
                }
            }
        }

        Ok(prunable_batch)
    }
}

fn estimate_highest_prunable_height(
    stored_head: &ExtendedHeader,
    pruning_cutoff: Time,
    block_time: Duration,
) -> Option<u64> {
    let blocks_until_cutoff = if pruning_cutoff < stored_head.time() {
        let time_until_cutoff = stored_head.time().duration_since(pruning_cutoff).unwrap();
        let blocks_until_cutoff = time_until_cutoff.as_nanos() / block_time.as_nanos();
        u64::try_from(blocks_until_cutoff).expect("unrealistic block time")
    } else {
        0
    };

    stored_head
        .height()
        .value()
        .checked_sub(blocks_until_cutoff)
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

    #[test]
    fn estimate_prunable_height() {
        let now = Time::now();
        let mut gen = ExtendedHeaderGenerator::new();

        let first_header_time = (now - Duration::from_secs(3 * 60)).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(6));

        let headers = gen.next_many(30);
        let head = headers.last().unwrap();
        let pruning_cutoff = now.checked_sub(Duration::from_secs(60)).unwrap();

        let mut found_height = None;

        for h in headers.iter().rev() {
            if h.time() <= pruning_cutoff {
                found_height = Some(h.height().value());
                break;
            }
        }

        let found_height = found_height.expect("Highest prunable height not found");
        let estimated_height =
            estimate_highest_prunable_height(head, pruning_cutoff, Duration::from_secs(6)).unwrap();

        assert!(estimated_height.abs_diff(found_height) <= 1);
    }

    #[test]
    fn estimate_prunable_height_beyond_genesis() {
        let now = Time::now();
        let mut gen = ExtendedHeaderGenerator::new();

        let first_header_time = (now - Duration::from_secs(3 * 60)).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(6));

        let headers = gen.next_many(30);
        let head = headers.last().unwrap();
        let pruning_cutoff = now.checked_sub(Duration::from_secs(5 * 60)).unwrap();

        // Pruning cutoff is beyond genesis
        assert_eq!(
            estimate_highest_prunable_height(head, pruning_cutoff, Duration::from_secs(6)),
            None
        );
    }

    #[test]
    fn estimate_prunable_height_after_head() {
        let now = Time::now();
        let mut gen = ExtendedHeaderGenerator::new();

        let first_header_time = (now - Duration::from_secs(3 * 60)).unwrap();
        gen.set_time(first_header_time, Duration::from_secs(6));

        let headers = gen.next_many(30);
        let head = headers.last().unwrap();
        let pruning_cutoff = now.checked_add(Duration::from_secs(1)).unwrap();

        // Pruning cutoff is after head
        assert_eq!(
            estimate_highest_prunable_height(head, pruning_cutoff, Duration::from_secs(6)),
            Some(head.height().value()),
        );
    }

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
            block_time: Duration::from_secs(1),
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
            block_time: Duration::from_secs(1),
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

        let blocks_with_sampling = (1..=500)
            .chain(601..=1000)
            .map(|height| {
                let block = TestBlock::from(height);
                let sampled = height % 3 == 0;
                (height, block, block.cid().unwrap(), sampled)
            })
            .collect::<Vec<_>>();

        store.insert(gen.next_many_verified(500)).await.unwrap();
        gen.skip(100);
        store.insert(gen.next_many_verified(400)).await.unwrap();

        for (height, block, cid, sampled) in &blocks_with_sampling {
            blockstore.put_keyed(cid, block.data()).await.unwrap();

            if *sampled {
                store.mark_as_sampled(*height).await.unwrap();
            }

            store
                .update_sampling_metadata(*height, vec![*cid])
                .await
                .unwrap();
        }

        let stored_ranges = store.get_stored_header_ranges().await.unwrap();
        let sampled_ranges = store.get_sampled_ranges().await.unwrap();

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore: blockstore.clone(),
            event_pub: events.publisher(),
            block_time: Duration::from_secs(1),
            pruning_window: DEFAULT_PRUNING_WINDOW,
            sampling_window: DEFAULT_SAMPLING_WINDOW,
        });

        for height in (601..=1000).rev().chain((1..=500).rev()) {
            assert!(stored_ranges.contains(height));

            // If a block is not sampled, Pruner asks Daser for permission to prune it.
            if !sampled_ranges.contains(height) {
                let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
                assert_eq!(want_to_prune, height);
                respond_to.send(true).unwrap();
            }
        }

        daser_handle.expect_no_cmd().await;

        assert_pruned_headers_event(&mut event_subscriber, 389, 500).await;
        assert_pruned_headers_event(&mut event_subscriber, 601, 1000).await;
        assert_pruned_headers_event(&mut event_subscriber, 1, 388).await;

        assert!(store.get_stored_header_ranges().await.unwrap().is_empty());

        for (height, _, cid, _) in &blocks_with_sampling {
            assert!(matches!(
                store.get_sampling_metadata(*height).await.unwrap_err(),
                StoreError::NotFound
            ));
            assert!(!blockstore.has(cid).await.unwrap());
        }

        pruner.stop();
        pruner.join().await;

        assert!(matches!(
            event_subscriber.try_recv().unwrap_err(),
            TryRecvError::Empty
        ));
    }

    #[async_test]
    async fn prune_tail() {
        let block_time = Duration::from_millis(1);
        let pruning_window = Duration::from_millis(4000);
        let sampling_window = Duration::from_millis(500);

        let events = EventChannel::new();
        let store = Arc::new(InMemoryStore::new());
        let mut gen = ExtendedHeaderGenerator::new();
        let blockstore = Arc::new(InMemoryBlockstore::new());
        let mut event_subscriber = events.subscribe();
        let (daser, mut daser_handle) = Daser::mocked();

        let first_header_time = (Time::now() - Duration::from_millis(5000)).unwrap();
        gen.set_time(first_header_time, block_time);

        // Tail
        store.insert(gen.next_many_verified(10)).await.unwrap();
        // Gap
        gen.skip(2480);
        // 10 headers within 1.5sec of pruning window edge
        store.insert(gen.next_many_verified(10)).await.unwrap();
        // Gap
        gen.skip(2490);
        // 10 headers at the current time
        store.insert(gen.next_many_verified(10)).await.unwrap();

        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([1..=10, 2491..=2500, 4991..=5000]),
        );

        let pruner = Pruner::start(PrunerArgs {
            daser: Arc::new(daser),
            store: store.clone(),
            blockstore,
            event_pub: events.publisher(),
            block_time,
            pruning_window,
            sampling_window,
        });

        for expected_height in (1..=10).rev() {
            let (height, respond_to) = daser_handle.expect_want_to_prune().await;
            assert_eq!(height, expected_height);
            respond_to.send(true).unwrap();
        }

        assert_pruned_headers_event(&mut event_subscriber, 1, 10).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([2491..=2500, 4991..=5000]),
        );

        sleep(Duration::from_millis(1500)).await;

        // Because Pruner runs in parallel it can generate 1 or 2 batches. The first
        // one will be the blocks that reached pruning edge while `sleep` is still running.
        //
        // Unfortunately we can not reliably predict the height that splits whose two
        // ranges, but it will be first one Pruner will send `WantToPrune` message.
        let (split_batch_height, respond_to) = daser_handle.expect_want_to_prune().await;
        respond_to.send(true).unwrap();

        // 1st batch
        for expected_height in (2491..=split_batch_height - 1).rev() {
            let (height, respond_to) = daser_handle.expect_want_to_prune().await;
            assert_eq!(height, expected_height);
            respond_to.send(true).unwrap();
        }

        // 2nd batch
        for expected_height in (split_batch_height + 1..=2500).rev() {
            let (height, respond_to) = daser_handle.expect_want_to_prune().await;
            assert_eq!(height, expected_height);
            respond_to.send(true).unwrap();
        }

        assert_pruned_headers_event(&mut event_subscriber, 2491, split_batch_height).await;
        // If it had a 2nd batch
        if split_batch_height < 2500 {
            assert_pruned_headers_event(&mut event_subscriber, split_batch_height + 1, 2500).await;
        }
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([4991..=5000]),
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
            new_block_ranges([4991..=5000])
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
            block_time: Duration::from_secs(1),
            pruning_window,
            sampling_window,
        });

        // Pruner asks from Daser if it can remove block 102.
        // We simulate that Daser allows it.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 102);
        respond_to.send(true).unwrap();

        // Pruner asks from Daser if it can remove block 101.
        // We simulate that Daser does not allow it.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 101);
        respond_to.send(false).unwrap();

        // Because Pruner runs in parallel with this test and removes in
        // batches, we expect the following:
        //
        // - Removes 102 because Daser allowed it.
        // - Keeps 101 because Daser didn't allow it.
        // - Consider blocks 188, 189, 192 as synced because they exists in pruned ranges.
        // - Remove 103-120, 147-154, 167-174, 187, 190-191, 193-195.
        // - Keep 146, 155, 166, 175, 186 blocks because they are edges
        //   (i.e. the blocks that are neightbors of non-synced ranges).
        // - Keep 196-199 because they are in sampling window and not sampled yet.
        // - Keep 202-260 because they are in pruning window.
        sleep(Duration::from_millis(100)).await;
        assert_pruned_headers_event(&mut event_subscriber, 1, 100).await;
        assert_pruned_headers_event(&mut event_subscriber, 102, 120).await;
        assert_pruned_headers_event(&mut event_subscriber, 147, 154).await;
        assert_pruned_headers_event(&mut event_subscriber, 167, 174).await;
        assert_pruned_headers_event(&mut event_subscriber, 187, 187).await;
        assert_pruned_headers_event(&mut event_subscriber, 190, 191).await;
        assert_pruned_headers_event(&mut event_subscriber, 193, 195).await;
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            new_block_ranges([
                101..=101,
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
            new_block_ranges([1..=100, 102..=120, 147..=154, 167..=174, 187..=195])
        );

        // Now Pruner will ask again for 101, but this time we allow it.
        let (want_to_prune, respond_to) = daser_handle.expect_want_to_prune().await;
        assert_eq!(want_to_prune, 101);
        respond_to.send(true).unwrap();

        assert_pruned_headers_event(&mut event_subscriber, 101, 101).await;
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
