//! Component responsible for data availability sampling of the already synchronized block
//! headers announced on the Celestia network.
//!
//! Sampling procedure comprises the following steps:
//!
//! 1. Daser waits for at least one peer to connect.
//! 2. Daser iterates in descending order over all the headers stored in the [`Store`].
//!     - If a block has not been sampled or it was rejected, Daser will queue it for sampling.
//!       Rejected blocks are resampled because their rejection could be caused by
//!       edge-cases unrelated to data availability, such as network issues.
//!     - If a block is not within the sampling window, it is not queued.
//!     - Queue is always sorted in descending order to give priority to latest blocks.
//! 3. As new headers become available in the [`Store`], Daser adds them to the queue if
//!    they are within the sampling window.
//! 4. If at any point new HEAD is queued, it is scheduled immediately and concurrently. Otherwise
//!    Daser waits for any ongoing sampling to finish and schedules a next block from the queue.
//!    Daser executes the following procedure for every scheduled block:
//!     - It makes sure that the block is still within the sampling window.
//!     - It selects which random shares are going to be sampled and generates their Shwap CIDs.
//!     - It updates [`Store`] with the CIDs that are going to be sampled. Tracking of the the CIDs
//!       is needed for pruning them later on. This is done before retrival of CIDs is started because
//!       otherwise user could stop the node after Bitswap stores the block in the blockstore, but before
//!       we can record that in the [`Store`], causing a leak.
//!     - Initiates Bitswap retrival requests for the specified CIDs.
//!     - If all CIDs are received, then the block is considered sampled and accepted.
//!     - If we reach a timeout of 10 seconds and at least one of the CIDs is not received, then
//!       block is considered sampled and rejected.
//!     - [`Store`] is updated with the sampling result.
//! 5. Steps 3 and 4 are repeated concurently, unless we detect that all peers have disconnected.
//!    At that point Daser cleans the queue and moves back to step 1.

use std::collections::HashSet;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use rand::Rng;
use tendermint::Time;
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};
use web_time::{Duration, Instant};

use lumina_utils::executor::{spawn, JoinHandle};
use lumina_utils::time::Interval;

use crate::events::{EventPublisher, NodeEvent};
use crate::p2p::shwap::sample_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{BlockRanges, Store, StoreError};
use crate::utils::OneshotSenderExt;

const MAX_SAMPLES_NEEDED: usize = 16;
const GET_SAMPLE_MIN_TIMEOUT: Duration = Duration::from_secs(10);

// TODO: Increase this when cberner/redb#970 is fixed
pub(crate) const DEFAULT_CONCURENCY_LIMIT: usize = 1;
pub(crate) const DEFAULT_ADDITIONAL_HEADER_SUB_CONCURENCY: usize = 5;

type Result<T, E = DaserError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur in `Daser` component.
#[derive(Debug, thiserror::Error)]
pub enum DaserError {
    /// An error propagated from the `P2p` component.
    #[error("P2p: {0}")]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Store`] component.
    #[error("Store: {0}")]
    Store(#[from] StoreError),

    /// The worker has died.
    #[error("Worker died")]
    WorkerDied,

    /// Channel closed unexpectedly.
    #[error("Channel closed unexpectedly")]
    ChannelClosedUnexpectedly,
}

impl From<oneshot::error::RecvError> for DaserError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        DaserError::ChannelClosedUnexpectedly
    }
}

/// Component responsible for data availability sampling of blocks from the network.
pub(crate) struct Daser {
    cmd_tx: mpsc::Sender<DaserCmd>,
    cancellation_token: CancellationToken,
    join_handle: JoinHandle,
}

/// Arguments used to configure the [`Daser`].
pub(crate) struct DaserArgs<S>
where
    S: Store,
{
    /// Handler for the peer to peer messaging.
    pub(crate) p2p: Arc<P2p>,
    /// Headers storage.
    pub(crate) store: Arc<S>,
    /// Event publisher.
    pub(crate) event_pub: EventPublisher,
    /// Size of the sampling window.
    pub(crate) sampling_window: Duration,
    /// How many blocks can be data sampled at the same time.
    pub(crate) concurrency_limit: usize,
    /// How many additional blocks can be data sampled if they are from HeaderSub.
    pub(crate) additional_headersub_concurrency: usize,
}

#[derive(Debug)]
pub(crate) enum DaserCmd {
    /// Used by Pruner to tell Daser about a block that is going to be pruned.
    /// Daser then replies with `true` if Pruner can do it. This is needed to
    /// avoid race conditions between them when Daser has as ongoing sampling.
    WantToPrune {
        height: u64,
        respond_to: oneshot::Sender<bool>,
    },
}

impl Daser {
    /// Create and start the [`Daser`].
    pub(crate) fn start<S>(args: DaserArgs<S>) -> Result<Self>
    where
        S: Store + 'static,
    {
        let cancellation_token = CancellationToken::new();
        let event_pub = args.event_pub.clone();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut worker = Worker::new(args, cancellation_token.child_token(), cmd_rx)?;

        let join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Daser stopped because of a fatal error: {e}");

                event_pub.send(NodeEvent::FatalDaserError {
                    error: e.to_string(),
                });
            }
        });

        Ok(Daser {
            cmd_tx,
            cancellation_token,
            join_handle,
        })
    }

    #[cfg(test)]
    pub(crate) fn mocked() -> (Self, crate::test_utils::MockDaserHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let cancellation_token = CancellationToken::new();

        // Just a fake join_handle
        let join_handle = spawn(async {});

        let daser = Daser {
            cmd_tx,
            cancellation_token,
            join_handle,
        };

        let mock_handle = crate::test_utils::MockDaserHandle { cmd_rx };

        (daser, mock_handle)
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

    async fn send_command(&self, cmd: DaserCmd) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| DaserError::WorkerDied)
    }

    pub(crate) async fn want_to_prune(&self, height: u64) -> Result<bool> {
        let (tx, rx) = oneshot::channel();

        self.send_command(DaserCmd::WantToPrune {
            height,
            respond_to: tx,
        })
        .await?;

        Ok(rx.await?)
    }
}

impl Drop for Daser {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Worker<S>
where
    S: Store + 'static,
{
    cmd_rx: mpsc::Receiver<DaserCmd>,
    cancellation_token: CancellationToken,
    event_pub: EventPublisher,
    p2p: Arc<P2p>,
    store: Arc<S>,
    max_samples_needed: usize,
    sampling_futs: FuturesUnordered<BoxFuture<'static, Result<(u64, bool)>>>,
    queue: BlockRanges,
    done: BlockRanges,
    ongoing: BlockRanges,
    will_be_pruned: BlockRanges,
    prev_head: Option<u64>,
    sampling_window: Duration,
    concurrency_limit: usize,
    additional_headersub_concurency: usize,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(
        args: DaserArgs<S>,
        cancellation_token: CancellationToken,
        cmd_rx: mpsc::Receiver<DaserCmd>,
    ) -> Result<Worker<S>> {
        Ok(Worker {
            cmd_rx,
            cancellation_token,
            event_pub: args.event_pub,
            p2p: args.p2p,
            store: args.store,
            max_samples_needed: MAX_SAMPLES_NEEDED,
            sampling_futs: FuturesUnordered::new(),
            queue: BlockRanges::default(),
            done: BlockRanges::default(),
            ongoing: BlockRanges::default(),
            will_be_pruned: BlockRanges::default(),
            prev_head: None,
            sampling_window: args.sampling_window,
            concurrency_limit: args.concurrency_limit,
            additional_headersub_concurency: args.additional_headersub_concurrency,
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
                Some(cmd) = self.cmd_rx.recv() => self.on_cmd(cmd).await,
            }
        }
    }

    async fn connected_event_loop(&mut self) -> Result<()> {
        debug!("Entering connected_event_loop");

        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
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

        let mut first_report = true;

        loop {
            // Start as many data sampling we are allowed.
            while self.schedule_next_sample_block().await? {}

            if first_report {
                self.report().await?;
                first_report = false;
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
                _ = report_interval.tick() => self.report().await?,
                Some(cmd) = self.cmd_rx.recv() => self.on_cmd(cmd).await,
                Some(res) = self.sampling_futs.next() => {
                    // Beetswap only returns fatal errors that are not related
                    // to P2P nor networking.
                    let (height, timed_out) = res?;

                    if !timed_out {
                        self.store.mark_as_sampled(height).await?;
                    }

                    self.ongoing.remove_relaxed(height..=height).expect("invalid height");
                    self.done.insert_relaxed(height..=height).expect("invalid height");
                },
                _ = &mut wait_new_head => {
                    wait_new_head = store.wait_new_head();
                    self.populate_queue().await?;
                }
            }
        }

        self.sampling_futs.clear();
        self.queue = BlockRanges::default();
        self.ongoing = BlockRanges::default();
        self.done = BlockRanges::default();
        self.prev_head = None;

        Ok(())
    }

    #[instrument(skip_all)]
    async fn report(&mut self) -> Result<()> {
        let sampled = self.store.get_sampled_ranges().await?;

        info!(
            "data sampling: stored and sampled blocks: {}, ongoing blocks: {}",
            sampled, &self.ongoing,
        );

        Ok(())
    }

    async fn on_cmd(&mut self, cmd: DaserCmd) {
        match cmd {
            DaserCmd::WantToPrune { height, respond_to } => {
                let res = self.on_want_to_prune(height).await;
                respond_to.maybe_send(res);
            }
        }
    }

    async fn on_want_to_prune(&mut self, height: u64) -> bool {
        // Pruner should not remove headers that are related to an ongoing sampling.
        if self.ongoing.contains(height) {
            return false;
        }

        // Header will be pruned, so we remove it from the queue to avoid race conditions.
        self.queue
            .remove_relaxed(height..=height)
            .expect("invalid height");
        // We also make sure `populate_queue` will not put it back.
        self.will_be_pruned
            .insert_relaxed(height..=height)
            .expect("invalid height");

        true
    }

    async fn schedule_next_sample_block(&mut self) -> Result<bool> {
        // Schedule the most recent un-sampled block.
        let header = loop {
            let Some(height) = self.queue.pop_head() else {
                return Ok(false);
            };

            // NOTE: This function runs on every iteration of Worker's main loop, so checking the
            // concurrency limit should be fast and without accessing the store. This is why we
            // do the calculation and the check here.
            //
            // Also, we do not update `self.prev_head` since `get_by_height` in this loop can fail.
            // We update it after the loop instead.
            let concurrency_limit = if self.prev_head.is_none_or(|prev_head| prev_head < height) {
                self.concurrency_limit + self.additional_headersub_concurency
            } else {
                self.concurrency_limit
            };

            if self.sampling_futs.len() >= concurrency_limit {
                // Put back the height we popped.
                self.queue
                    .insert_relaxed(height..=height)
                    .expect("invalid height");
                return Ok(false);
            }

            match self.store.get_by_height(height).await {
                Ok(header) => break header,
                Err(StoreError::NotFound) => {
                    // Height was pruned and our queue is inconsistent.
                    // Repopulate queue and try again.
                    self.populate_queue().await?;
                }
                Err(e) => return Err(e.into()),
            }
        };

        let height = header.height().value();
        let square_width = header.dah.square_width();

        // New head found (check also the logic above).
        if self.prev_head.is_none_or(|prev_head| prev_head < height) {
            self.prev_head = Some(height);
        }

        // Make sure that the block is still in the sampling window.
        if !self.in_sampling_window(header.time()) {
            // As soon as we reach a block that is not in the sampling
            // window, it means the rest wouldn't be either.
            self.queue
                .remove_relaxed(1..=height)
                .expect("invalid height");
            self.done
                .insert_relaxed(1..=height)
                .expect("invalid height");
            return Ok(false);
        }

        // Select random shares to be sampled
        let share_indexes = random_indexes(square_width, self.max_samples_needed);

        // Update the CID list before we start sampling, otherwise it's possible for us
        // to leak CIDs causing associated blocks to never get cleaned from blockstore.
        let cids = share_indexes
            .iter()
            .map(|(row, col)| sample_cid(*row, *col, height))
            .collect::<Result<Vec<_>, _>>()?;
        self.store.update_sampling_metadata(height, cids).await?;

        let p2p = self.p2p.clone();
        let event_pub = self.event_pub.clone();
        let sampling_window = self.sampling_window;

        // Schedule retrival of the CIDs. This will be run later on in the `select!` loop.
        let fut = async move {
            let now = Instant::now();

            event_pub.send(NodeEvent::SamplingStarted {
                height,
                square_width,
                shares: share_indexes.iter().copied().collect(),
            });

            // We set the timeout high enough until block goes out of sampling window.
            let timeout = calc_timeout(header.time(), Time::now(), sampling_window);

            // Initialize all futures
            let mut futs = share_indexes
                .into_iter()
                .map(|(row, col)| {
                    let p2p = p2p.clone();

                    async move {
                        let res = p2p.get_sample(row, col, height, Some(timeout)).await;
                        (row, col, res)
                    }
                })
                .collect::<FuturesUnordered<_>>();

            let mut sampling_timed_out = false;

            // Run futures to completion
            while let Some((row, column, res)) = futs.next().await {
                let timed_out = match res {
                    Ok(_) => false,
                    // Validation is done at Bitswap level, through `ShwapMultihasher`.
                    // If the sample is not valid, it will never be delivered to us
                    // as the data of the CID. Because of that, the only signal
                    // that data sampling verification failed is query timing out.
                    Err(P2pError::BitswapQueryTimeout) => true,
                    Err(e) => return Err(e.into()),
                };

                if timed_out {
                    sampling_timed_out = true;
                }

                event_pub.send(NodeEvent::ShareSamplingResult {
                    height,
                    square_width,
                    row,
                    column,
                    timed_out,
                });
            }

            event_pub.send(NodeEvent::SamplingResult {
                height,
                timed_out: sampling_timed_out,
                took: now.elapsed(),
            });

            Ok((height, sampling_timed_out))
        }
        .boxed();

        self.sampling_futs.push(fut);
        self.ongoing
            .insert_relaxed(height..=height)
            .expect("invalid height");

        Ok(true)
    }

    /// Add to the queue the blocks that need to be sampled.
    ///
    /// NOTE: We resample rejected blocks, because rejection can happen
    /// in some unrelated edge-cases, such us network issues. This is a Shwap
    /// limitation that's coming from bitswap: only way for us to know if sampling
    /// failed is via timeout.
    async fn populate_queue(&mut self) -> Result<()> {
        let stored = self.store.get_stored_header_ranges().await?;
        let sampled = self.store.get_sampled_ranges().await?;

        self.queue = stored - sampled - &self.done - &self.ongoing - &self.will_be_pruned;

        Ok(())
    }

    /// Returns true if `time` is within the sampling window.
    fn in_sampling_window(&self, time: Time) -> bool {
        let now = Time::now();

        // Header is from the future! Thus, within sampling window.
        if now < time {
            return true;
        }

        let Ok(age) = now.duration_since(time) else {
            return false;
        };

        age <= self.sampling_window
    }
}

fn calc_timeout(header_time: Time, now: Time, sampling_window: Duration) -> Duration {
    let sampling_window_end = now.checked_sub(sampling_window).unwrap_or_else(|| {
        warn!("underflow when computing sampling window, defaulting to unix epoch");
        Time::unix_epoch()
    });

    let timeout = header_time
        .duration_since(sampling_window_end)
        .unwrap_or(GET_SAMPLE_MIN_TIMEOUT);

    timeout.max(GET_SAMPLE_MIN_TIMEOUT)
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
    use crate::node::DEFAULT_SAMPLING_WINDOW;
    use crate::p2p::shwap::convert_cid;
    use crate::p2p::P2pCmd;
    use crate::store::InMemoryStore;
    use crate::test_utils::MockP2pHandle;
    use bytes::BytesMut;
    use celestia_proto::bitswap::Block;
    use celestia_types::consts::appconsts::AppVersion;
    use celestia_types::sample::{Sample, SampleId};
    use celestia_types::test_utils::{generate_dummy_eds, ExtendedHeaderGenerator};
    use celestia_types::{AxisType, DataAvailabilityHeader, ExtendedDataSquare};
    use cid::Cid;
    use lumina_utils::test_utils::async_test;
    use lumina_utils::time::sleep;
    use prost::Message;
    use std::collections::HashMap;
    use std::time::Duration;

    // Request number for which tests will simulate sampling timeout
    //
    // NOTE: The smallest block has 4 shares, so a 2nd request will always happen.
    const REQ_TIMEOUT_SHARE_NUM: usize = 2;

    #[async_test]
    async fn check_calc_timeout() {
        let now = Time::now();
        let sampling_window = Duration::from_secs(60);

        let header_time = now;
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            Duration::from_secs(60)
        );

        let header_time = now.checked_sub(Duration::from_secs(1)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            Duration::from_secs(59)
        );

        let header_time = now.checked_sub(Duration::from_secs(49)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            Duration::from_secs(11)
        );

        let header_time = now.checked_sub(Duration::from_secs(50)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            Duration::from_secs(10)
        );

        // minimum timeout edge 1
        let header_time = now.checked_sub(Duration::from_secs(51)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            GET_SAMPLE_MIN_TIMEOUT
        );

        // minimum timeout edge 2
        let header_time = now.checked_sub(Duration::from_secs(60)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            GET_SAMPLE_MIN_TIMEOUT
        );

        // header outside of the sampling window
        let header_time = now.checked_sub(Duration::from_secs(61)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            GET_SAMPLE_MIN_TIMEOUT
        );

        // header from the "future"
        let header_time = now.checked_add(Duration::from_secs(1)).unwrap();
        assert_eq!(
            calc_timeout(header_time, now, sampling_window),
            Duration::from_secs(61)
        );
    }

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
            sampling_window: DEFAULT_SAMPLING_WINDOW,
            concurrency_limit: 1,
            additional_headersub_concurrency: DEFAULT_ADDITIONAL_HEADER_SUB_CONCURENCY,
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
    async fn sampling_timeout() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());
        let events = EventChannel::new();
        let mut event_sub = events.subscribe();

        let _daser = Daser::start(DaserArgs {
            event_pub: events.publisher(),
            p2p: Arc::new(mock),
            store: store.clone(),
            sampling_window: DEFAULT_SAMPLING_WINDOW,
            concurrency_limit: 1,
            additional_headersub_concurrency: DEFAULT_ADDITIONAL_HEADER_SUB_CONCURENCY,
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
            sampling_window: DEFAULT_SAMPLING_WINDOW,
            concurrency_limit: 1,
            additional_headersub_concurrency: DEFAULT_ADDITIONAL_HEADER_SUB_CONCURENCY,
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;

        let mut edses = Vec::new();
        let mut headers = Vec::new();

        for _ in 0..20 {
            let eds = generate_dummy_eds(2, AppVersion::V2);
            let dah = DataAvailabilityHeader::from_eds(&eds);
            let header = gen.next_with_dah(dah);

            edses.push(eds);
            headers.push(header);
        }

        // Insert 5-10 block headers
        store.insert(headers[4..=9].to_vec()).await.unwrap();

        // Sample block 10
        handle_get_shwap_cid(&mut handle, 10, &edses[9], false).await;

        // Sample block 9
        handle_get_shwap_cid(&mut handle, 9, &edses[8], false).await;

        // To avoid race conditions we wait a bit for the block 8 to be scheduled
        sleep(Duration::from_millis(10)).await;

        // Insert 16-20 block headers
        store.insert(headers[15..=19].to_vec()).await.unwrap();

        // To avoid race conditions we wait a bit for the new head (block 20) to be scheduled
        sleep(Duration::from_millis(10)).await;

        // Now daser runs two concurrent data sampling: block 8 and block 20
        handle_concurrent_get_shwap_cid(
            &mut handle,
            [(8, &edses[9], false), (20, &edses[19], false)],
        )
        .await;

        // Sample and reject block 19
        handle_get_shwap_cid(&mut handle, 19, &edses[18], true).await;

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
        handle_get_shwap_cid(&mut handle, 19, &edses[18], false).await;

        // Sample block 16 until 18
        for height in (16..=18).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(&mut handle, height, &edses[idx], false).await;
        }

        // Sample block 5 until 7
        for height in (5..=7).rev() {
            let idx = height as usize - 1;
            handle_get_shwap_cid(&mut handle, height, &edses[idx], false).await;
        }

        handle.expect_no_cmd().await;

        // Push block 21 in the store
        let eds = generate_dummy_eds(2, AppVersion::V2);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        store.insert(header).await.unwrap();

        // Sample block 21
        handle_get_shwap_cid(&mut handle, 21, &eds, false).await;

        handle.expect_no_cmd().await;
    }

    async fn gen_and_sample_block(
        handle: &mut MockP2pHandle,
        gen: &mut ExtendedHeaderGenerator,
        store: &InMemoryStore,
        event_sub: &mut EventSubscriber,
        square_width: usize,
        simulate_sampling_timeout: bool,
    ) {
        let eds = generate_dummy_eds(square_width, AppVersion::V2);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        let height = header.height().value();

        store.insert(header).await.unwrap();

        let cids = handle_get_shwap_cid(handle, height, &eds, simulate_sampling_timeout).await;
        handle.expect_no_cmd().await;

        // Check if block was sampled or timed-out.
        let sampled_ranges = store.get_sampled_ranges().await.unwrap();
        assert_eq!(sampled_ranges.contains(height), !simulate_sampling_timeout);

        // Check if CIDs we requested successfully made it in the store
        let mut sampling_metadata = store.get_sampling_metadata(height).await.unwrap().unwrap();
        sampling_metadata.cids.sort();
        assert_eq!(&sampling_metadata.cids, &cids);

        // Check if we received `SamplingStarted` event
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
                    timed_out,
                } => {
                    assert_eq!(ev_height, height);
                    assert_eq!(square_width, eds.square_width());
                    assert_eq!(
                        timed_out,
                        simulate_sampling_timeout && i == REQ_TIMEOUT_SHARE_NUM
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
            NodeEvent::SamplingResult {
                height: ev_height,
                timed_out,
                took,
            } => {
                assert_eq!(ev_height, height);
                assert_eq!(timed_out, simulate_sampling_timeout);
                assert_ne!(took, Duration::default());
            }
            ev => panic!("Unexpected event: {ev}"),
        }

        assert!(event_sub.try_recv().is_err());
    }

    /// Responds to get_shwap_cid and returns all CIDs that were requested
    async fn handle_concurrent_get_shwap_cid<const N: usize>(
        handle: &mut MockP2pHandle,
        handling_args: [(u64, &ExtendedDataSquare, bool); N],
    ) -> Vec<Cid> {
        struct Info<'a> {
            eds: &'a ExtendedDataSquare,
            simulate_sampling_timeout: bool,
            needed_samples: usize,
            requests_count: usize,
        }

        let mut infos = handling_args
            .into_iter()
            .map(|(height, eds, simulate_sampling_timeout)| {
                let square_width = eds.square_width() as usize;
                let needed_samples = (square_width * square_width).min(MAX_SAMPLES_NEEDED);

                (
                    height,
                    Info {
                        eds,
                        simulate_sampling_timeout,
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

            // Simulate sampling timeout
            if info.simulate_sampling_timeout && info.requests_count == REQ_TIMEOUT_SHARE_NUM {
                respond_to.send(Err(P2pError::BitswapQueryTimeout)).unwrap();
                continue;
            }

            let sample = gen_sample_of_cid(sample_id, info.eds).await;
            respond_to.send(Ok(sample)).unwrap();
        }

        cids.sort();
        cids
    }

    /// Responds to get_shwap_cid and returns all CIDs that were requested
    async fn handle_get_shwap_cid(
        handle: &mut MockP2pHandle,
        height: u64,
        eds: &ExtendedDataSquare,
        simulate_sampling_timeout: bool,
    ) -> Vec<Cid> {
        handle_concurrent_get_shwap_cid(handle, [(height, eds, simulate_sampling_timeout)]).await
    }

    async fn gen_sample_of_cid(sample_id: SampleId, eds: &ExtendedDataSquare) -> Vec<u8> {
        let sample = Sample::new(
            sample_id.row_index(),
            sample_id.column_index(),
            AxisType::Row,
            eds,
        )
        .unwrap();

        let mut container = BytesMut::new();
        sample.encode(&mut container);

        let block = Block {
            cid: convert_cid(&sample_id.into()).unwrap().to_bytes(),
            container: container.to_vec(),
        };

        block.encode_to_vec()
    }
}
