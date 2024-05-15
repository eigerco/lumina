//! Component responsible for data availability sampling of the already synchronized block
//! headers announced on the Celestia network.
//!
//! Sampling procedure comprises the following steps:
//!
//! 1. Daser waits for at least one peer to connect.
//! 2. Daser gets the local head height from the [`Store`] and iterates in reverse order
//!    over all the stored blocks.
//!    - If a block has not been sampled or it was rejected, Daser will queue it for sampling.
//!      Rejected blocks are resampled because their rejection could be caused by
//!      edge-cases unrelated to data availability, such as network issues.
//!    - If a block is not within the sampling window, it is not queued.
//!    - Queue is always sorted in descending order to give priority to latest blocks.
//! 3. As new headers become available in the [`Store`], Daser adds them to the queue if
//!    they are within the sampling window.
//! 4. If there isn't any ongoing sampling, Daser pops from the queue the next block to sample
//!    and initiates the following procedure:
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
//! 5. Steps 3 and 4 are repeated concurently, unless we detect that all peers have disconnected.
//!    At that point Daser cleans the queue and moves back to step 1.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use celestia_tendermint::Time;
use celestia_types::ExtendedHeader;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use instant::{Duration, Instant};
use rand::Rng;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::events::{EventPublisher, NodeEvent};
use crate::executor::spawn;
use crate::p2p::shwap::sample_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{SamplingStatus, Store, StoreError};
use crate::utils::FusedReusableBoxFuture;

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
    sampling_fut: FusedReusableBoxFuture<Result<(u64, bool)>>,
    queue: VecDeque<SamplingArgs>,
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
            sampling_fut: FusedReusableBoxFuture::terminated(),
            queue: VecDeque::new(),
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

        self.populate_queue().await;

        loop {
            if self.sampling_fut.is_terminated() {
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
                res = &mut self.sampling_fut => {
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
                _ = self.store.wait_new_head() => {
                    self.populate_queue().await;
                }
            }
        }

        self.sampling_fut.terminate();
        self.queue.clear();
        self.prev_head = None;

        Ok(())
    }

    async fn schedule_next_sample_block(&mut self) -> Result<()> {
        assert!(self.sampling_fut.is_terminated());

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
        self.sampling_fut.set(async move {
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
        });

        Ok(())
    }

    /// Add to the queue the blocks that need to be sampled.
    ///
    /// NOTE: We resample rejected blocks, because rejection can happen
    /// in some unrelated edge-cases, such us network issues. This is a Shwap
    /// limitation that's coming from bitswap: only way for us to know if sampling
    /// failed is via timeout.
    async fn populate_queue(&mut self) {
        // TODO: Adjust algorithm for backward syncing.

        let Ok(new_head) = self.store.head_height().await else {
            return;
        };

        match self.prev_head {
            Some(prev_head) => {
                for height in prev_head + 1..=new_head {
                    if is_block_accepted(&*self.store, height).await {
                        continue;
                    }

                    let Ok(header) = self.store.get_by_height(height).await else {
                        // Maybe header was pruned, continue to the next one.
                        continue;
                    };

                    if !in_sampling_window(header.time()) {
                        // Block is older than the sampling window, continue to the next one.
                        continue;
                    }

                    queue_sampling(&mut self.queue, &header);
                }
            }
            None => {
                // Reverse iterate heights
                for height in (1..=new_head).rev() {
                    if is_block_accepted(&*self.store, height).await {
                        continue;
                    }

                    let Ok(header) = self.store.get_by_height(height).await else {
                        // We reached the tail of the known heights
                        break;
                    };

                    if !in_sampling_window(header.time()) {
                        // We've reached the tail of the sampling window
                        break;
                    }

                    queue_sampling(&mut self.queue, &header);
                }
            }
        }

        self.prev_head = Some(new_head);
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
    use crate::executor::{sleep, timeout};
    use crate::p2p::P2pCmd;
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, MockP2pHandle};
    use celestia_tendermint_proto::Protobuf;
    use celestia_types::sample::{Sample, SampleId};
    use celestia_types::test_utils::{generate_eds, ExtendedHeaderGenerator};
    use celestia_types::{AxisType, DataAvailabilityHeader, ExtendedDataSquare};
    use cid::Cid;
    use std::time::Duration;

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

        let mut edses = Vec::new();
        let mut headers = Vec::new();

        for _ in 0..20 {
            let eds = generate_eds(2);
            let dah = DataAvailabilityHeader::from_eds(&eds);
            let header = gen.next_with_dah(dah);

            edses.push(eds);
            headers.push(header);
        }

        store.append(headers[0..10].to_vec()).await.unwrap();
        // Wait a bit for sampling of block 10 start
        sleep(Duration::from_millis(10)).await;
        store.append(headers[10..].to_vec()).await.unwrap();
        // Wait a bit for the queue to be populated with higher blocks
        sleep(Duration::from_millis(10)).await;

        // Sample block 10
        handle_get_shwap_cid(&mut handle, &store, &mut event_sub, 10, &edses[9], false).await;

        // Sample block 20
        handle_get_shwap_cid(&mut handle, &store, &mut event_sub, 20, &edses[19], false).await;

        // Sample and reject block 19
        handle_get_shwap_cid(&mut handle, &store, &mut event_sub, 19, &edses[18], true).await;

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
        // Discard also any events that were generated.
        while try_recv_event(&mut event_sub).await.is_some() {}

        // We shouldn't have any other requests from daser because it's in connecting state.
        handle.expect_no_cmd().await;

        // Simulate that a peer connected
        handle.announce_peer_connected();

        // Because of disconnection, daser will resample block 19
        handle_get_shwap_cid(&mut handle, &store, &mut event_sub, 19, &edses[18], false).await;

        // Sample block 18 until 11
        for height in (11..=18).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(
                &mut handle,
                &store,
                &mut event_sub,
                height,
                &edses[idx],
                false,
            )
            .await;
        }

        // Sample block 9 until 1
        for height in (1..=9).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(
                &mut handle,
                &store,
                &mut event_sub,
                height,
                &edses[idx],
                false,
            )
            .await;
        }

        handle.expect_no_cmd().await;

        // Push block 21 in the store
        let eds = generate_eds(2);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        store.append_single(header).await.unwrap();

        // Sample block 21
        handle_get_shwap_cid(&mut handle, &store, &mut event_sub, 21, &eds, false).await;

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

        store.append_single(header).await.unwrap();

        let cids = handle_get_shwap_cid(
            handle,
            store,
            event_sub,
            height,
            &eds,
            simulate_invalid_sampling,
        )
        .await;
        handle.expect_no_cmd().await;

        let sampling_metadata = store.get_sampling_metadata(height).await.unwrap().unwrap();

        if simulate_invalid_sampling {
            assert_eq!(sampling_metadata.status, SamplingStatus::Rejected);
        } else {
            assert_eq!(sampling_metadata.status, SamplingStatus::Accepted);
        }

        assert_eq!(sampling_metadata.cids, cids);
    }

    /// Responds to get_shwap_cid and returns all CIDs that were requested
    async fn handle_get_shwap_cid(
        handle: &mut MockP2pHandle,
        store: &InMemoryStore,
        event_sub: &mut EventSubscriber,
        height: u64,
        eds: &ExtendedDataSquare,
        simulate_invalid_sampling: bool,
    ) -> Vec<Cid> {
        let square_width = eds.square_width() as usize;
        let needed_samples = (square_width * square_width).min(MAX_SAMPLES_NEEDED);

        // Check if we get the correct event
        let mut remaining_shares = match expect_event(event_sub).await {
            NodeEvent::SamplingStarted {
                height: ev_height,
                square_width: ev_square_width,
                shares,
            } => {
                assert_eq!(ev_height, height);
                assert_eq!(ev_square_width as usize, square_width);
                shares.into_iter().collect::<HashSet<_>>()
            }
            ev => panic!("Unexpected event: {ev}"),
        };
        assert_eq!(remaining_shares.len(), needed_samples);

        let mut cids = Vec::with_capacity(needed_samples);

        for i in 0..needed_samples {
            let (cid, respond_to) = handle.expect_get_shwap_cid().await;
            cids.push(cid);

            let sample_id: SampleId = cid.try_into().unwrap();
            assert_eq!(sample_id.block_height(), height);

            // Check if share was in the event
            assert!(remaining_shares.remove(&(sample_id.row_index(), sample_id.column_index())));

            // Simulate invalid sample by triggering BitswapQueryTimeout
            if simulate_invalid_sampling && i == 2 {
                respond_to.send(Err(P2pError::BitswapQueryTimeout)).unwrap();

                // Check if we get the correct event
                match expect_event(event_sub).await {
                    NodeEvent::ShareSamplingResult {
                        height: ev_height,
                        square_width: ev_square_width,
                        row,
                        column,
                        accepted,
                    } => {
                        assert_eq!(ev_height, height);
                        assert_eq!(ev_square_width as usize, square_width);
                        assert_eq!(row, sample_id.row_index());
                        assert_eq!(column, sample_id.column_index());
                        assert!(!accepted);
                    }
                    ev => panic!("Unexpected event: {ev}"),
                }

                continue;
            }

            let sample = gen_sample_of_cid(sample_id, eds, store).await;
            let sample_bytes = sample.encode_vec().unwrap();

            respond_to.send(Ok(sample_bytes)).unwrap();

            // Check if we get the correct event
            match expect_event(event_sub).await {
                NodeEvent::ShareSamplingResult {
                    height: ev_height,
                    square_width: ev_square_width,
                    row,
                    column,
                    accepted,
                } => {
                    assert_eq!(ev_height, height);
                    assert_eq!(ev_square_width as usize, square_width);
                    assert_eq!(row, sample_id.row_index());
                    assert_eq!(column, sample_id.column_index());
                    assert!(accepted);
                }
                ev => panic!("Unexpected event: {ev}"),
            }
        }

        // Check if we get the correct event
        match expect_event(event_sub).await {
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

        cids
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

    async fn try_recv_event(event_sub: &mut EventSubscriber) -> Option<NodeEvent> {
        timeout(Duration::from_millis(300), async move {
            event_sub.recv().await.unwrap().event
        })
        .await
        .ok()
    }

    async fn expect_event(event_sub: &mut EventSubscriber) -> NodeEvent {
        try_recv_event(event_sub)
            .await
            .expect("Expecting NodeEvent, but timed-out")
    }
}
