//! Component responsible for data availability sampling of the already synchronized block
//! headers announced on the Celestia network.
//!
//! Sampling procedure comprises the following steps:
//!
//! 1. Daser waits for at least one peer to connect.
//! 2. Daser iterates in descending order over all stored headers of the [`Store`].
//!    - If a block has not been sampled or it was rejected, Daser will queue it for sampling.
//!      Rejected blocks are resampled because their rejection could be caused by
//!      edge-cases unrelated to data availability, such as network issues.
//!    - If a block is not within the sampling window, it is not queued.
//!    - Queue is always sorted in descending order to give priority to latest blocks.
//! 3. As new headers become available in the [`Store`], Daser adds them to the queue if
//!    they are within the sampling window.
//! 4. If new HEAD was queued, it is scheduled immediately and concurently.
//! 5. If there isn't any ongoing sampling, the next block from the queue is scheduled.
//! 6. Daser initiates the following procedure for every scheduled block:
//!    - It makes sure that the block is still within the sampling window.
//!    - It selects which random shares are going to be sampled and generates their Shwap CIDs.
//!    - It updates [`Store`] with the CIDs that are going to be sampled. Tracking of the the CIDs
//!      is needed for pruning them later on. This is done before retrival of CIDs is started because otherwise
//!      user could stop the node after Bitswap stores the block in the blockstore, but before we can
//!      record that in the [`Store`], causing a leak.
//!    - Initiates Bitswap retrival requests for the specified CIDs.
//!    - If all CIDs are received, then the block is considered sampled and accepted.
//!    - If we reach a timeout of 10 seconds and at least one of the CIDs is not received, then
//!      block is considered sampled and rejected.
//!    - [`Store`] is updated with the sampling result.
//! 7. Steps 3 and 4-5-6 are repeated concurently, unless we detect that all peers have disconnected.
//!    At that point Daser cleans the queue and moves back to step 1.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use celestia_tendermint::Time;
use celestia_types::ExtendedHeader;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use rand::Rng;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};
use web_time::{Duration, Instant};

use crate::events::{EventPublisher, NodeEvent};
use crate::executor::spawn;
use crate::p2p::shwap::sample_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{HeaderRanges, SamplingStatus, Store, StoreError};

const MAX_SAMPLES_NEEDED: usize = 16;

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;
const SAMPLING_WINDOW: Duration = Duration::from_secs(30 * DAY);

type Result<T, E = DaserError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with the [`Daser`].
#[derive(Debug, thiserror::Error)]
pub enum DaserError {
    /// An error propagated from the [`P2p`] module.
    #[error(transparent)]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Store`] module.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Component responsible for data availability sampling of blocks from the network.
pub struct Daser {
    cancellation_token: CancellationToken,
}

/// Arguments used to configure the [`Daser`].
pub struct DaserArgs<S>
where
    S: Store,
{
    /// Handler for the peer to peer messaging.
    pub p2p: Arc<P2p>,
    /// Headers storage.
    pub store: Arc<S>,
    /// Event publisher.
    pub event_pub: EventPublisher,
}

impl Daser {
    /// Create and start the [`Daser`].
    pub fn start<S>(args: DaserArgs<S>) -> Result<Self>
    where
        S: Store + 'static,
    {
        let cancellation_token = CancellationToken::new();
        let event_pub = args.event_pub.clone();
        let mut worker = Worker::new(args, cancellation_token.child_token())?;

        spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Daser stopped because of a fatal error: {e}");

                event_pub.send(NodeEvent::FatalDaserError {
                    error: e.to_string(),
                });
            }
        });

        Ok(Daser { cancellation_token })
    }

    /// Stop the [`Daser`].
    pub fn stop(&self) {
        // Singal the Worker to stop.
        // TODO: Should we wait for the Worker to stop?
        self.cancellation_token.cancel();
    }
}

impl Drop for Daser {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

struct Worker<S>
where
    S: Store + 'static,
{
    cancellation_token: CancellationToken,
    event_pub: EventPublisher,
    p2p: Arc<P2p>,
    store: Arc<S>,
    max_samples_needed: usize,
    sampling_futs: FuturesUnordered<BoxFuture<'static, Result<(u64, bool)>>>,
    queue: VecDeque<SamplingArgs>,
    prev_stored_blocks: HeaderRanges,
    prev_head: Option<u64>,
}

#[derive(Debug)]
struct SamplingArgs {
    height: u64,
    square_width: u16,
    time: Time,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(args: DaserArgs<S>, cancellation_token: CancellationToken) -> Result<Worker<S>> {
        Ok(Worker {
            cancellation_token,
            event_pub: args.event_pub,
            p2p: args.p2p,
            store: args.store,
            max_samples_needed: MAX_SAMPLES_NEEDED,
            sampling_futs: FuturesUnordered::new(),
            queue: VecDeque::new(),
            prev_stored_blocks: HeaderRanges::default(),
            prev_head: None,
        })
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connecting_event_loop().await;

            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connected_event_loop().await?;
        }

        debug!("Daser stopped");
        Ok(())
    }

    async fn connecting_event_loop(&mut self) {
        debug!("Entering connecting_event_loop");

        let mut peer_tracker_info_watcher = self.p2p.peer_tracker_info_watcher();

        // Check if connection status changed before watcher was created
        if peer_tracker_info_watcher.borrow().num_connected_peers > 0 {
            return;
        }

        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    break;
                }
                _ = peer_tracker_info_watcher.changed() => {
                    if peer_tracker_info_watcher.borrow().num_connected_peers > 0 {
                        break;
                    }
                }
            }
        }
    }

    async fn connected_event_loop(&mut self) -> Result<()> {
        debug!("Entering connected_event_loop");

        let mut peer_tracker_info_watcher = self.p2p.peer_tracker_info_watcher();

        // Check if connection status changed before the watcher was created
        if peer_tracker_info_watcher.borrow().num_connected_peers == 0 {
            warn!("All peers disconnected");
            return Ok(());
        }

        // Workaround because `wait_new_head` is not cancel-safe.
        //
        // TODO: Only Syncer add new headers to the store, so ideally
        // Syncer should inform Daser that new headers were added.
        let store = self.store.clone();
        let mut wait_new_head = store.wait_new_head();

        self.populate_queue().await?;

        loop {
            // If we have a new HEAD queued, schedule it now!
            if let Some(queue_front) = self.queue.front().map(|args| args.height) {
                if queue_front > self.prev_head.unwrap_or(0) {
                    self.schedule_next_sample_block().await?;
                    self.prev_head = Some(queue_front);
                }
            }

            // If there is no ongoing data sampling, schedule the next one.
            if self.sampling_futs.is_empty() {
                self.schedule_next_sample_block().await?;
            }

            select! {
                _ = self.cancellation_token.cancelled() => {
                    break;
                }
                _ = peer_tracker_info_watcher.changed() => {
                    if peer_tracker_info_watcher.borrow().num_connected_peers == 0 {
                        warn!("All peers disconnected");
                        break;
                    }
                }
                Some(res) = self.sampling_futs.next() => {
                    // Beetswap only returns fatal errors that are not related
                    // to P2P nor networking.
                    let (height, accepted) = res?;

                    let status = if accepted {
                        SamplingStatus::Accepted
                    } else {
                        SamplingStatus::Rejected
                    };

                    self.store
                        .update_sampling_metadata(height, status, Vec::new())
                        .await?;
                },
                _ = &mut wait_new_head => {
                    wait_new_head = store.wait_new_head();
                    self.populate_queue().await?;
                }
            }
        }

        self.sampling_futs.clear();
        self.queue.clear();
        self.prev_stored_blocks = HeaderRanges::default();
        self.prev_head = None;

        Ok(())
    }

    async fn schedule_next_sample_block(&mut self) -> Result<()> {
        let Some(args) = self.queue.pop_front() else {
            return Ok(());
        };

        // Make sure that the block is still in the sampling window.
        if !in_sampling_window(args.time) {
            // Queue is sorted by block height in descending order,
            // so as soon as we reach a block that is not in the sampling
            // window, it means the rest wouldn't be either.
            self.queue.clear();
            return Ok(());
        }

        // Select random shares to be sampled
        let share_indexes = random_indexes(args.square_width, self.max_samples_needed);

        // Update the CID list before we start sampling, otherwise it's possible for us
        // to leak CIDs causing associated blocks to never get cleaned from blockstore.
        let cids = share_indexes
            .iter()
            .map(|(row, col)| sample_cid(*row, *col, args.height))
            .collect::<Result<Vec<_>, _>>()?;
        self.store
            .update_sampling_metadata(args.height, SamplingStatus::Unknown, cids)
            .await?;

        let p2p = self.p2p.clone();
        let event_pub = self.event_pub.clone();

        // Schedule retrival of the CIDs. This will be run later on in the `select!` loop.
        let fut = async move {
            let now = Instant::now();

            event_pub.send(NodeEvent::SamplingStarted {
                height: args.height,
                square_width: args.square_width,
                shares: share_indexes.iter().copied().collect(),
            });

            // Initialize all futures
            let mut futs = share_indexes
                .into_iter()
                .map(|(row, col)| {
                    let p2p = p2p.clone();

                    async move {
                        let res = p2p.get_sample(row, col, args.height).await;
                        (row, col, res)
                    }
                })
                .collect::<FuturesUnordered<_>>();

            let mut block_accepted = true;

            // Run futures to completion
            while let Some((row, column, res)) = futs.next().await {
                let share_accepted = match res {
                    Ok(_) => true,
                    // Validation is done at Bitswap level, through `ShwapMultihasher`.
                    // If the sample is not valid, it will never be delivered to us
                    // as the data of the CID. Because of that, the only signal
                    // that data sampling verification failed is query timing out.
                    Err(P2pError::BitswapQueryTimeout) => false,
                    Err(e) => return Err(e.into()),
                };

                block_accepted &= share_accepted;

                event_pub.send(NodeEvent::ShareSamplingResult {
                    height: args.height,
                    square_width: args.square_width,
                    row,
                    column,
                    accepted: share_accepted,
                });
            }

            event_pub.send(NodeEvent::SamplingFinished {
                height: args.height,
                accepted: block_accepted,
                took: now.elapsed(),
            });

            Ok((args.height, block_accepted))
        }
        .boxed();

        self.sampling_futs.push(fut);

        Ok(())
    }

    /// Add to the queue the blocks that need to be sampled.
    ///
    /// NOTE: We resample rejected blocks, because rejection can happen
    /// in some unrelated edge-cases, such us network issues. This is a Shwap
    /// limitation that's coming from bitswap: only way for us to know if sampling
    /// failed is via timeout.
    async fn populate_queue(&mut self) -> Result<()> {
        let stored_blocks = self.store.get_stored_header_ranges().await?;
        let first_check = self.prev_stored_blocks.is_empty();

        'outer: for block_range in stored_blocks.clone().into_inner().into_iter().rev() {
            for height in block_range.rev() {
                if self.prev_stored_blocks.contains(height) {
                    // Skip blocks that were checked before
                    continue;
                }

                // Optimization: We check if the block was accepted only if this is
                // the first time we check the store (i.e. prev_stored_blocks is empty),
                // otherwise we can safely assume that block needs sampling.
                if first_check && is_block_accepted(&*self.store, height).await {
                    // Skip already sampled blocks
                    continue;
                }

                let Ok(header) = self.store.get_by_height(height).await else {
                    // We reached the tail of the known heights
                    break 'outer;
                };

                if !in_sampling_window(header.time()) {
                    // We've reached the tail of the sampling window
                    break 'outer;
                }

                queue_sampling(&mut self.queue, &header);
            }
        }

        self.prev_stored_blocks = stored_blocks;

        Ok(())
    }
}

/// Queue sampling in descending order
fn queue_sampling(queue: &mut VecDeque<SamplingArgs>, header: &ExtendedHeader) {
    let args = SamplingArgs {
        height: header.height().value(),
        time: header.time(),
        square_width: header.dah.square_width(),
    };

    if queue.is_empty() || args.height >= queue.front().unwrap().height {
        queue.push_front(args);
        return;
    }

    if args.height <= queue.back().unwrap().height {
        queue.push_back(args);
        return;
    }

    let pos = match queue.binary_search_by(|x| args.height.cmp(&x.height)) {
        Ok(pos) => pos,
        Err(pos) => pos,
    };

    queue.insert(pos, args);
}

/// Returns true if `time` is within the sampling window.
fn in_sampling_window(time: Time) -> bool {
    let now = Time::now();

    // Header is from the future! Thus, within sampling window.
    if now < time {
        return true;
    }

    let Ok(age) = now.duration_since(time) else {
        return false;
    };

    age <= SAMPLING_WINDOW
}

/// Returns true if block has been sampled and accepted.
async fn is_block_accepted(store: &impl Store, height: u64) -> bool {
    match store.get_sampling_metadata(height).await {
        Ok(Some(metadata)) => metadata.status == SamplingStatus::Accepted,
        _ => false,
    }
}

/// Returns unique and random indexes that will be used for sampling.
fn random_indexes(square_width: u16, max_samples_needed: usize) -> HashSet<(u16, u16)> {
    let samples_in_block = usize::from(square_width).pow(2);

    // If block size is smaller than `max_samples_needed`, we are going
    // to sample the whole block. Randomness is not needed for this.
    if samples_in_block <= max_samples_needed {
        return (0..square_width)
            .flat_map(|row| (0..square_width).map(move |col| (row, col)))
            .collect();
    }

    let mut indexes = HashSet::with_capacity(max_samples_needed);
    let mut rng = rand::thread_rng();

    while indexes.len() < max_samples_needed {
        let row = rng.gen::<u16>() % square_width;
        let col = rng.gen::<u16>() % square_width;
        indexes.insert((row, col));
    }

    indexes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventChannel, EventSubscriber};
    use crate::executor::sleep;
    use crate::p2p::P2pCmd;
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, MockP2pHandle};
    use celestia_tendermint_proto::Protobuf;
    use celestia_types::sample::{Sample, SampleId};
    use celestia_types::test_utils::{generate_eds, ExtendedHeaderGenerator};
    use celestia_types::{AxisType, DataAvailabilityHeader, ExtendedDataSquare};
    use cid::Cid;
    use std::collections::HashMap;
    use std::time::Duration;

    // Request number for which tests will simulate invalid sampling
    //
    // NOTE: The smallest block has 4 shares, so a 2nd request will always happen.
    const INVALID_SHARE_REQ_NUM: usize = 2;

    #[async_test]
    async fn received_valid_samples() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());
        let events = EventChannel::new();
        let mut event_sub = events.subscribe();

        let _daser = Daser::start(DaserArgs {
            event_pub: events.publisher(),
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;

        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 2, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 4, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 8, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 16, false).await;
    }

    #[async_test]
    async fn received_invalid_sample() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());
        let events = EventChannel::new();
        let mut event_sub = events.subscribe();

        let _daser = Daser::start(DaserArgs {
            event_pub: events.publisher(),
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;

        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 2, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 4, true).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, &mut event_sub, 8, false).await;
    }

    #[async_test]
    async fn backward_dasing() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());
        let events = EventChannel::new();

        let _daser = Daser::start(DaserArgs {
            event_pub: events.publisher(),
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;

        let mut edses = Vec::new();
        let mut headers = Vec::new();

        for _ in 0..20 {
            let eds = generate_eds(2);
            let dah = DataAvailabilityHeader::from_eds(&eds);
            let header = gen.next_with_dah(dah);

            edses.push(eds);
            headers.push(header);
        }

        // Insert 5-10 block headers
        store.insert(headers[4..=9].to_vec()).await.unwrap();

        // Sample block 10
        handle_get_shwap_cid(&mut handle, &store, 10, &edses[9], false).await;

        // Sample block 9
        handle_get_shwap_cid(&mut handle, &store, 9, &edses[8], false).await;

        // To avoid race conditions we wait a bit for the block 8 to be scheduled
        sleep(Duration::from_millis(10)).await;

        // Insert 16-20 block headers
        store.insert(headers[15..=19].to_vec()).await.unwrap();

        // To avoid race conditions we wait a bit for the new head (block 20) to be scheduled
        sleep(Duration::from_millis(10)).await;

        // Now daser runs two concurrent data sampling: block 8 and block 20
        handle_concurrent_get_shwap_cid(
            &mut handle,
            &store,
            [(8, &edses[9], false), (20, &edses[19], false)],
        )
        .await;

        // Sample and reject block 19
        handle_get_shwap_cid(&mut handle, &store, 19, &edses[18], true).await;

        // Simulate disconnection
        handle.announce_all_peers_disconnected();

        // Daser may scheduled Block 18 already, so we need to reply to that requests.
        // For the sake of the test we reply with a bitswap timeout.
        while let Some(cmd) = handle.try_recv_cmd().await {
            match cmd {
                P2pCmd::GetShwapCid { respond_to, .. } => {
                    let _ = respond_to.send(Err(P2pError::BitswapQueryTimeout));
                }
                cmd => panic!("Unexpected command: {cmd:?}"),
            }
        }

        // We shouldn't have any other requests from daser because it's in connecting state.
        handle.expect_no_cmd().await;

        // Simulate that a peer connected
        handle.announce_peer_connected();

        // Because of disconnection and previous rejection of block 19, daser will resample it
        handle_get_shwap_cid(&mut handle, &store, 19, &edses[18], false).await;

        // Sample block 16 until 18
        for height in (16..=18).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(&mut handle, &store, height, &edses[idx], false).await;
        }

        // Sample block 5 until 7
        for height in (5..=7).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(&mut handle, &store, height, &edses[idx], false).await;
        }

        handle.expect_no_cmd().await;

        // Push block 21 in the store
        let eds = generate_eds(2);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        store.insert(header).await.unwrap();

        // Sample block 21
        handle_get_shwap_cid(&mut handle, &store, 21, &eds, false).await;

        handle.expect_no_cmd().await;
    }

    async fn gen_and_sample_block(
        handle: &mut MockP2pHandle,
        gen: &mut ExtendedHeaderGenerator,
        store: &InMemoryStore,
        event_sub: &mut EventSubscriber,
        square_width: usize,
        simulate_invalid_sampling: bool,
    ) {
        let eds = generate_eds(square_width);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        let height = header.height().value();

        store.insert(header).await.unwrap();

        let cids =
            handle_get_shwap_cid(handle, store, height, &eds, simulate_invalid_sampling).await;
        handle.expect_no_cmd().await;

        let mut sampling_metadata = store.get_sampling_metadata(height).await.unwrap().unwrap();
        sampling_metadata.cids.sort();

        if simulate_invalid_sampling {
            assert_eq!(sampling_metadata.status, SamplingStatus::Rejected);
        } else {
            assert_eq!(sampling_metadata.status, SamplingStatus::Accepted);
        }

        // Check if CIDs we received successfully made it in the store
        assert_eq!(&sampling_metadata.cids, &cids);

        // Check if received `SamplingStarted` event
        let mut remaining_shares = match event_sub.try_recv().unwrap().event {
            NodeEvent::SamplingStarted {
                height: ev_height,
                square_width,
                shares,
            } => {
                assert_eq!(ev_height, height);
                assert_eq!(square_width, eds.square_width());

                // Make sure the share list matches the CIDs we received
                let mut cids = shares
                    .iter()
                    .map(|(row, col)| sample_cid(*row, *col, height).unwrap())
                    .collect::<Vec<_>>();
                cids.sort();
                assert_eq!(&sampling_metadata.cids, &cids);

                shares.into_iter().collect::<HashSet<_>>()
            }
            ev => panic!("Unexpected event: {ev}"),
        };

        // Check if we received `ShareSamplingResult` for each share
        for i in 1..=remaining_shares.len() {
            match event_sub.try_recv().unwrap().event {
                NodeEvent::ShareSamplingResult {
                    height: ev_height,
                    square_width,
                    row,
                    column,
                    accepted,
                } => {
                    assert_eq!(ev_height, height);
                    assert_eq!(square_width, eds.square_width());
                    assert_eq!(
                        accepted,
                        !(simulate_invalid_sampling && i == INVALID_SHARE_REQ_NUM)
                    );
                    // Make sure it is in the list and remove it
                    assert!(remaining_shares.remove(&(row, column)));
                }
                ev => panic!("Unexpected event: {ev}"),
            }
        }

        assert!(remaining_shares.is_empty());

        // Check if we received `SamplingFinished` for each share
        match event_sub.try_recv().unwrap().event {
            NodeEvent::SamplingFinished {
                height: ev_height,
                accepted,
                took,
            } => {
                assert_eq!(ev_height, height);
                assert_eq!(accepted, !simulate_invalid_sampling);
                assert_ne!(took, Duration::default());
            }
            ev => panic!("Unexpected event: {ev}"),
        }

        assert!(event_sub.try_recv().is_err());
    }

    /// Responds to get_shwap_cid and returns all CIDs that were requested
    async fn handle_concurrent_get_shwap_cid<const N: usize>(
        handle: &mut MockP2pHandle,
        store: &InMemoryStore,
        handling_args: [(u64, &ExtendedDataSquare, bool); N],
    ) -> Vec<Cid> {
        struct Info<'a> {
            eds: &'a ExtendedDataSquare,
            simulate_invalid_sampling: bool,
            needed_samples: usize,
            requests_count: usize,
        }

        let mut infos = handling_args
            .into_iter()
            .map(|(height, eds, simulate_invalid_sampling)| {
                let square_width = eds.square_width() as usize;
                let needed_samples = (square_width * square_width).min(MAX_SAMPLES_NEEDED);

                (
                    height,
                    Info {
                        eds,
                        simulate_invalid_sampling,
                        needed_samples,
                        requests_count: 0,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        let needed_samples_sum = infos.values().map(|info| info.needed_samples).sum();
        let mut cids = Vec::with_capacity(needed_samples_sum);

        for _ in 0..needed_samples_sum {
            let (cid, respond_to) = handle.expect_get_shwap_cid().await;
            cids.push(cid);

            let sample_id: SampleId = cid.try_into().unwrap();
            let info = infos
                .get_mut(&sample_id.block_height())
                .unwrap_or_else(|| panic!("Unexpected height: {}", sample_id.block_height()));

            info.requests_count += 1;

            // Simulate invalid sample by triggering BitswapQueryTimeout
            if info.simulate_invalid_sampling && info.requests_count == INVALID_SHARE_REQ_NUM {
                respond_to.send(Err(P2pError::BitswapQueryTimeout)).unwrap();
                continue;
            }

            let sample = gen_sample_of_cid(sample_id, info.eds, store).await;
            let sample_bytes = sample.encode_vec().unwrap();

            respond_to.send(Ok(sample_bytes)).unwrap();
        }

        cids.sort();
        cids
    }

    /// Responds to get_shwap_cid and returns all CIDs that were requested
    async fn handle_get_shwap_cid(
        handle: &mut MockP2pHandle,
        store: &InMemoryStore,
        height: u64,
        eds: &ExtendedDataSquare,
        simulate_invalid_sampling: bool,
    ) -> Vec<Cid> {
        handle_concurrent_get_shwap_cid(handle, store, [(height, eds, simulate_invalid_sampling)])
            .await
    }

    async fn gen_sample_of_cid(
        sample_id: SampleId,
        eds: &ExtendedDataSquare,
        store: &InMemoryStore,
    ) -> Sample {
        let header = store.get_by_height(sample_id.block_height()).await.unwrap();

        Sample::new(
            sample_id.row_index(),
            sample_id.column_index(),
            AxisType::Row,
            eds,
            header.height().value(),
        )
        .unwrap()
    }
}
