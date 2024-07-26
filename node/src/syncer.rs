//! Component responsible for synchronizing block headers announced in the Celestia network.
//!
//! It starts by asking the trusted peers for their current head headers and picks
//! the latest header returned by at least two of them as the initial synchronization target
//! called `subjective_head`.
//!
//! Then it starts synchronizing from the genesis header up to the target requesting headers
//! on the `header-ex` p2p protocol. In the meantime, it constantly checks for the latest
//! headers announced on the `header-sub` p2p protocol to keep the `subjective_head` as close
//! to the `network_head` as possible.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use celestia_tendermint::Time;
use celestia_types::ExtendedHeader;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};
use web_time::Instant;

use crate::block_ranges::{BlockRange, BlockRangeExt, BlockRanges};
use crate::events::{EventPublisher, NodeEvent};
use crate::executor::{sleep, spawn, spawn_cancellable, Interval};
use crate::p2p::{P2p, P2pError};
use crate::store::utils::calculate_range_to_fetch;
use crate::store::{Store, StoreError};
use crate::utils::OneshotSenderExt;

type Result<T, E = SyncerError> = std::result::Result<T, E>;

const TRY_INIT_BACKOFF_MAX_INTERVAL: Duration = Duration::from_secs(60);
const SYNCING_WINDOW: Duration = Duration::from_secs(30 * 24 * 60 * 60); // 30 days

/// Representation of all the errors that can occur in `Syncer` component.
#[derive(Debug, thiserror::Error)]
pub enum SyncerError {
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

impl SyncerError {
    pub(crate) fn is_fatal(&self) -> bool {
        match self {
            SyncerError::P2p(e) => e.is_fatal(),
            SyncerError::Store(e) => e.is_fatal(),
            SyncerError::WorkerDied | SyncerError::ChannelClosedUnexpectedly => true,
        }
    }
}

impl From<oneshot::error::RecvError> for SyncerError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        SyncerError::ChannelClosedUnexpectedly
    }
}

/// Component responsible for synchronizing block headers from the network.
#[derive(Debug)]
pub(crate) struct Syncer<S>
where
    S: Store + 'static,
{
    cmd_tx: mpsc::Sender<SyncerCmd>,
    cancellation_token: CancellationToken,
    _store: PhantomData<S>,
}

/// Arguments used to configure the [`Syncer`].
pub(crate) struct SyncerArgs<S>
where
    S: Store + 'static,
{
    /// Handler for the peer to peer messaging.
    pub(crate) p2p: Arc<P2p>,
    /// Headers storage.
    pub(crate) store: Arc<S>,
    /// Event publisher.
    pub(crate) event_pub: EventPublisher,
    /// Batch size.
    pub(crate) batch_size: u64,
}

#[derive(Debug)]
enum SyncerCmd {
    GetInfo {
        respond_to: oneshot::Sender<SyncingInfo>,
    },
}

/// Status of the synchronization.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncingInfo {
    /// Ranges of headers that are already synchronised
    pub stored_headers: BlockRanges,
    /// Syncing target. The latest height seen in the network that was successfully verified.
    pub subjective_head: u64,
}

impl<S> Syncer<S>
where
    S: Store,
{
    /// Create and start the [`Syncer`].
    pub(crate) fn start(args: SyncerArgs<S>) -> Result<Self> {
        let cancellation_token = CancellationToken::new();
        let event_pub = args.event_pub.clone();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut worker = Worker::new(args, cancellation_token.child_token(), cmd_rx)?;

        spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Syncer stopped because of a fatal error: {e}");

                event_pub.send(NodeEvent::FatalSyncerError {
                    error: e.to_string(),
                });
            }
        });

        Ok(Syncer {
            cancellation_token,
            cmd_tx,
            _store: PhantomData,
        })
    }

    /// Stop the worker.
    pub(crate) fn stop(&self) {
        // Singal the Worker to stop.
        // TODO: Should we wait for the Worker to stop?
        self.cancellation_token.cancel();
    }

    async fn send_command(&self, cmd: SyncerCmd) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| SyncerError::WorkerDied)
    }

    /// Get the current synchronization status.
    ///
    /// # Errors
    ///
    /// This function will return an error if the [`Syncer`] has been stopped.
    pub(crate) async fn info(&self) -> Result<SyncingInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_command(SyncerCmd::GetInfo { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }
}

impl<S> Drop for Syncer<S>
where
    S: Store,
{
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

struct Worker<S>
where
    S: Store + 'static,
{
    cancellation_token: CancellationToken,
    cmd_rx: mpsc::Receiver<SyncerCmd>,
    event_pub: EventPublisher,
    p2p: Arc<P2p>,
    store: Arc<S>,
    header_sub_rx: Option<mpsc::Receiver<ExtendedHeader>>,
    subjective_head_height: Option<u64>,
    batch_size: u64,
    ongoing_batch: Option<Ongoing>,
    estimated_syncing_window_end: Option<u64>,
}

struct Ongoing {
    batch: BlockRange,
    cancellation_token: CancellationToken,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(
        args: SyncerArgs<S>,
        cancellation_token: CancellationToken,
        cmd_rx: mpsc::Receiver<SyncerCmd>,
    ) -> Result<Self> {
        Ok(Worker {
            cancellation_token,
            cmd_rx,
            event_pub: args.event_pub,
            p2p: args.p2p,
            store: args.store,
            header_sub_rx: None,
            subjective_head_height: None,
            batch_size: args.batch_size,
            ongoing_batch: None,
            estimated_syncing_window_end: None,
        })
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connecting_event_loop().await?;

            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connected_event_loop().await?;
        }

        debug!("Syncer stopped");
        Ok(())
    }

    /// The responsibility of this event loop is to await a trusted peer to
    /// connect and get the network head, while accepting commands.
    ///
    /// NOTE: Only fatal errors should be propagated!
    async fn connecting_event_loop(&mut self) -> Result<()> {
        debug!("Entering connecting_event_loop");

        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
        let mut try_init_result = self.spawn_try_init().fuse();

        self.report().await?;

        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    break;
                }
                _ = report_interval.tick() => {
                    self.report().await?;
                }
                res = &mut try_init_result => {
                    // try_init task propagates only fatal errors
                    let (network_head, took) = res??;
                    let network_head_height = network_head.height().value();

                    info!("Setting initial subjective head to {network_head_height}");
                    self.set_subjective_head_height(network_head_height);

                    let (header_sub_tx, header_sub_rx) = mpsc::channel(16);
                    self.p2p.init_header_sub(network_head, header_sub_tx).await?;
                    self.header_sub_rx = Some(header_sub_rx);

                    self.event_pub.send(NodeEvent::FetchingHeadHeaderFinished {
                        height: network_head_height,
                        took,
                    });

                    break;
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    self.on_cmd(cmd).await?;
                }
            }
        }

        Ok(())
    }

    /// The reponsibility of this event loop is to start the syncing process,
    /// handles events from HeaderSub, and accept commands.
    ///
    /// NOTE: Only fatal errors should be propagated!
    async fn connected_event_loop(&mut self) -> Result<()> {
        debug!("Entering connected_event_loop");

        let (headers_tx, mut headers_rx) = mpsc::channel(1);
        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
        let mut peer_tracker_info_watcher = self.p2p.peer_tracker_info_watcher();

        // Check if connection status changed before creating the watcher
        if peer_tracker_info_watcher.borrow().num_connected_peers == 0 {
            warn!("All peers disconnected");
            return Ok(());
        }

        self.fetch_next_batch(&headers_tx).await?;
        self.report().await?;

        loop {
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
                _ = report_interval.tick() => {
                    self.report().await?;
                }
                res = header_sub_recv(self.header_sub_rx.as_mut()) => {
                    let header = res?;
                    self.on_header_sub_message(header).await?;
                    self.fetch_next_batch(&headers_tx).await?;
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    self.on_cmd(cmd).await?;
                }
                Some((res, took)) = headers_rx.recv() => {
                    self.on_fetch_next_batch_result(res, took).await?;
                    self.fetch_next_batch(&headers_tx).await?;
                }
            }
        }

        if let Some(ongoing) = self.ongoing_batch.take() {
            warn!("Cancelling fetching of {}", ongoing.batch.display());
            ongoing.cancellation_token.cancel();
        }

        self.header_sub_rx.take();

        Ok(())
    }

    async fn syncing_info(&self) -> Result<SyncingInfo> {
        Ok(SyncingInfo {
            stored_headers: self.store.get_stored_header_ranges().await?,
            subjective_head: self.subjective_head_height.unwrap_or(0),
        })
    }

    #[instrument(skip_all)]
    async fn report(&mut self) -> Result<()> {
        let SyncingInfo {
            stored_headers,
            subjective_head,
        } = self.syncing_info().await?;

        let ongoing_batch = self
            .ongoing_batch
            .as_ref()
            .map(|ongoing| format!("{}", ongoing.batch.display()))
            .unwrap_or_else(|| "None".to_string());

        info!("syncing: head: {subjective_head}, stored headers: {stored_headers}, ongoing batches: {ongoing_batch}");
        Ok(())
    }

    fn spawn_try_init(&self) -> oneshot::Receiver<Result<(ExtendedHeader, Duration)>> {
        let p2p = self.p2p.clone();
        let store = self.store.clone();
        let event_pub = self.event_pub.clone();
        let (tx, rx) = oneshot::channel();

        let fut = async move {
            let mut event_reported = false;
            let now = Instant::now();
            let mut backoff = ExponentialBackoffBuilder::default()
                .with_max_interval(TRY_INIT_BACKOFF_MAX_INTERVAL)
                .with_max_elapsed_time(None)
                .build();

            loop {
                match try_init(&p2p, &*store, &event_pub, &mut event_reported).await {
                    Ok(network_head) => {
                        tx.maybe_send(Ok((network_head, now.elapsed())));
                        break;
                    }
                    Err(e) if e.is_fatal() => {
                        tx.maybe_send(Err(e));
                        break;
                    }
                    Err(e) => {
                        let sleep_dur = backoff
                            .next_backoff()
                            .expect("backoff never stops retrying");

                        warn!("Initialization of subjective head failed: {e}. Trying again in {sleep_dur:?}.");
                        sleep(sleep_dur).await;
                    }
                }
            }
        };

        spawn_cancellable(
            self.cancellation_token.child_token(),
            fut.instrument(info_span!("try_init")),
        );

        rx
    }

    async fn on_cmd(&mut self, cmd: SyncerCmd) -> Result<()> {
        match cmd {
            SyncerCmd::GetInfo { respond_to } => {
                let info = self.syncing_info().await?;
                respond_to.maybe_send(info);
            }
        }

        Ok(())
    }

    #[instrument(skip_all)]
    async fn on_header_sub_message(&mut self, new_head: ExtendedHeader) -> Result<()> {
        let new_head_height = new_head.height().value();

        self.set_subjective_head_height(new_head_height);

        if let Ok(store_head_height) = self.store.head_height().await {
            // If our new header is adjacent to the HEAD of the store
            if store_head_height + 1 == new_head_height {
                // Header is already verified by HeaderSub and will be validated against previous
                // head on insert
                if self.store.insert(new_head).await.is_ok() {
                    self.event_pub.send(NodeEvent::AddedHeaderFromHeaderSub {
                        height: new_head_height,
                    });
                }
            }
        }

        Ok(())
    }

    fn set_subjective_head_height(&mut self, height: u64) {
        if let Some(old_height) = self.subjective_head_height {
            if height <= old_height {
                return;
            }
        }

        self.subjective_head_height = Some(height);
    }

    #[instrument(skip_all)]
    async fn fetch_next_batch(
        &mut self,
        headers_tx: &mpsc::Sender<(Result<Vec<ExtendedHeader>, P2pError>, Duration)>,
    ) -> Result<()> {
        if self.ongoing_batch.is_some() {
            // Another batch is ongoing. We do not parallelize `Syncer`
            // by design. Any parallel requests are done in the
            // HeaderEx client through `Session`.
            //
            // Nothing to schedule
            return Ok(());
        }

        if self.p2p.peer_tracker_info().num_connected_peers == 0 {
            // No connected peers. We can't do the request.
            // We will recover from this in `run`.
            return Ok(());
        }

        let Some(subjective_head_height) = self.subjective_head_height else {
            // Nothing to schedule
            return Ok(());
        };

        let store_ranges = self.store.get_stored_header_ranges().await?;

        let next_batch = calculate_range_to_fetch(
            subjective_head_height,
            store_ranges.as_ref(),
            self.estimated_syncing_window_end,
            self.batch_size,
        );

        if next_batch.is_empty() {
            // no headers to fetch
            return Ok(());
        }

        // make sure we're inside the syncing window before we start
        match self.store.get_by_height(next_batch.end() + 1).await {
            Ok(known_header) => {
                if !in_syncing_window(&known_header) {
                    self.estimated_syncing_window_end = Some(known_header.height().value());
                    return Ok(());
                }
            }
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }

        self.event_pub.send(NodeEvent::FetchingHeadersStarted {
            from_height: *next_batch.start(),
            to_height: *next_batch.end(),
        });

        let cancellation_token = self.cancellation_token.child_token();

        self.ongoing_batch = Some(Ongoing {
            batch: next_batch.clone(),
            cancellation_token: cancellation_token.clone(),
        });

        let tx = headers_tx.clone();
        let p2p = self.p2p.clone();

        spawn_cancellable(cancellation_token, async move {
            let now = Instant::now();
            let res = p2p.get_unverified_header_range(next_batch).await;
            let _ = tx.send((res, now.elapsed())).await;
        });

        Ok(())
    }

    /// Handle the result of the batch request and propagate fatal errors.
    #[instrument(skip_all)]
    async fn on_fetch_next_batch_result(
        &mut self,
        res: Result<Vec<ExtendedHeader>, P2pError>,
        took: Duration,
    ) -> Result<()> {
        let Some(ongoing) = self.ongoing_batch.take() else {
            warn!("No batch was scheduled, however result was received. Discarding it.");
            return Ok(());
        };

        let from_height = *ongoing.batch.start();
        let to_height = *ongoing.batch.end();

        let headers = match res {
            Ok(headers) => headers,
            Err(e) => {
                if e.is_fatal() {
                    return Err(e.into());
                }

                self.event_pub.send(NodeEvent::FetchingHeadersFailed {
                    from_height,
                    to_height,
                    error: e.to_string(),
                    took,
                });

                return Ok(());
            }
        };

        if let Err(e) = self.store.insert(headers).await {
            if e.is_fatal() {
                return Err(e.into());
            }

            self.event_pub.send(NodeEvent::FetchingHeadersFailed {
                from_height,
                to_height,
                error: format!("Failed to store headers: {e}"),
                took,
            });
        }

        self.event_pub.send(NodeEvent::FetchingHeadersFinished {
            from_height,
            to_height,
            took,
        });

        Ok(())
    }
}

fn in_syncing_window(header: &ExtendedHeader) -> bool {
    let syncing_window_start = Time::now().checked_sub(SYNCING_WINDOW).unwrap_or_else(|| {
        warn!("underflow when computing syncing window start, defaulting to unix epoch");
        Time::unix_epoch()
    });

    header.time().after(syncing_window_start)
}

async fn try_init<S>(
    p2p: &P2p,
    store: &S,
    event_pub: &EventPublisher,
    event_reported: &mut bool,
) -> Result<ExtendedHeader>
where
    S: Store,
{
    p2p.wait_connected_trusted().await?;

    if !*event_reported {
        event_pub.send(NodeEvent::FetchingHeadHeaderStarted);
        *event_reported = true;
    }

    let network_head = p2p.get_head_header().await?;

    // If the network head and the store head have the same height,
    // then `insert` will error because of insertion contraints.
    // However, if both headers are the exactly the same, we
    // can skip inserting, as the header is already there.
    //
    // This can happen in case of fast node restart.
    let try_insert = match store.get_head().await {
        Ok(store_head) => store_head != network_head,
        Err(StoreError::NotFound) => true,
        Err(e) => return Err(e.into()),
    };

    if try_insert {
        // Insert HEAD to the store and initialize header-sub.
        // Normal insertion checks still apply here.
        store.insert(network_head.clone()).await?;
    }

    Ok(network_head)
}

async fn header_sub_recv(
    rx: Option<&mut mpsc::Receiver<ExtendedHeader>>,
) -> Result<ExtendedHeader> {
    rx.expect("header-sub not initialized")
        .recv()
        .await
        .ok_or(SyncerError::P2p(P2pError::WorkerDied))
}

#[cfg(test)]
mod tests {
    use std::ops::RangeInclusive;

    use super::*;
    use crate::block_ranges::{BlockRange, BlockRangeExt};
    use crate::events::EventChannel;
    use crate::p2p::header_session;
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, gen_filled_store, MockP2pHandle};
    use celestia_types::test_utils::ExtendedHeaderGenerator;

    #[async_test]
    async fn init_without_genesis_hash() {
        let events = EventChannel::new();
        let (mock, mut handle) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let header = gen.next();

        let _syncer = Syncer::start(SyncerArgs {
            p2p: Arc::new(mock),
            store: Arc::new(InMemoryStore::new()),
            event_pub: events.publisher(),
            batch_size: 512,
        })
        .unwrap();

        // Syncer is waiting for a trusted peer to connect
        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;
        handle.announce_trusted_peer_connected();

        // We're syncing from the front, so ask for head first
        let (height, amount, respond_to) = handle.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![header.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = handle.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, header);

        // network head = local head, so nothing else is produced.
        handle.expect_no_cmd().await;
    }

    #[async_test]
    async fn init_with_genesis_hash() {
        let mut gen = ExtendedHeaderGenerator::new();
        let head = gen.next();

        let (_syncer, _store, mut p2p_mock) = initialized_syncer(head.clone()).await;

        // network head = local head, so nothing else is produced.
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn syncing() {
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(26);

        let (syncer, store, mut p2p_mock) = initialized_syncer(headers[25].clone()).await;
        assert_syncing(&syncer, &store, &[26..=26], 26).await;

        // Syncer will sync all headers up to the head
        handle_session_batch(&mut p2p_mock, &headers, 1..=25, true).await;
        assert_syncing(&syncer, &store, &[1..=26], 26).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;

        // New HEAD was received by HeaderSub (height 27)
        let header27 = gen.next();
        p2p_mock.announce_new_head(header27.clone());
        // Height 27 is adjacent to the last header of Store, so it is appended
        // immediately
        assert_syncing(&syncer, &store, &[1..=27], 27).await;
        p2p_mock.expect_no_cmd().await;

        // New HEAD was received by HeaderSub (height 30), it should NOT be appended
        let header_28_30 = gen.next_many(3);
        p2p_mock.announce_new_head(header_28_30[2].clone());
        assert_syncing(&syncer, &store, &[1..=27], 30).await;

        // New HEAD is not adjacent to store, so Syncer requests a range
        handle_session_batch(&mut p2p_mock, &header_28_30, 28..=30, true).await;
        assert_syncing(&syncer, &store, &[1..=30], 30).await;

        // New HEAD was received by HeaderSub (height 1058), it SHOULD be appended as it's adjacent
        let mut headers = gen.next_many(1028);
        p2p_mock.announce_new_head(headers.last().cloned().unwrap());
        assert_syncing(&syncer, &store, &[1..=30], 1058).await;

        // Syncer requested the first batch
        handle_session_batch(&mut p2p_mock, &headers, 547..=1058, true).await;
        assert_syncing(&syncer, &store, &[1..=30, 547..=1058], 1058).await;

        // New head from header sub added
        headers.push(gen.next());
        p2p_mock.announce_new_head(headers.last().cloned().unwrap());
        assert_syncing(&syncer, &store, &[1..=30, 547..=1059], 1059).await;

        // Syncer requested the second batch
        handle_session_batch(&mut p2p_mock, &headers, 35..=546, true).await;
        assert_syncing(&syncer, &store, &[1..=30, 35..=1059], 1059).await;

        // Syncer requested the last batch
        handle_session_batch(&mut p2p_mock, &headers, 31..=34, true).await;
        assert_syncing(&syncer, &store, &[1..=1059], 1059).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn syncing_window_edge() {
        let month_and_day_ago = Duration::from_secs(31 * 24 * 60 * 60);
        let mut gen = ExtendedHeaderGenerator::new();
        gen.set_time((Time::now() - month_and_day_ago).expect("to not underflow"));
        let mut headers = gen.next_many(1200);
        gen.set_time(Time::now());
        headers.append(&mut gen.next_many(2049 - 1200));

        let (syncer, store, mut p2p_mock) = initialized_syncer(headers[2048].clone()).await;
        assert_syncing(&syncer, &store, &[2049..=2049], 2049).await;

        // Syncer requested the first batch
        handle_session_batch(&mut p2p_mock, &headers, 1537..=2048, true).await;
        assert_syncing(&syncer, &store, &[1537..=2049], 2049).await;

        // Syncer requested the second batch hitting the syncing window
        handle_session_batch(&mut p2p_mock, &headers, 1025..=1536, true).await;
        assert_syncing(&syncer, &store, &[1025..=2049], 2049).await;

        // Syncer is fully synced and awaiting for events
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn start_with_filled_store() {
        let events = EventChannel::new();
        let (p2p, mut p2p_mock) = P2p::mocked();
        let (store, mut gen) = gen_filled_store(25).await;
        let store = Arc::new(store);

        let mut headers = gen.next_many(520);
        let network_head = gen.next(); // height 546

        let syncer = Syncer::start(SyncerArgs {
            p2p: Arc::new(p2p),
            store: store.clone(),
            event_pub: events.publisher(),
            batch_size: 512,
        })
        .unwrap();

        p2p_mock.announce_trusted_peer_connected();

        // Syncer asks for current HEAD
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![network_head.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = p2p_mock.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, network_head);

        assert_syncing(&syncer, &store, &[1..=25, 546..=546], 546).await;

        // Syncer requested the first batch
        handle_session_batch(&mut p2p_mock, &headers, 34..=545, true).await;
        assert_syncing(&syncer, &store, &[1..=25, 34..=546], 546).await;

        // Syncer requested the remaining batch ([26, 33])
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 26);
        assert_eq!(amount, 8);
        respond_to
            .send(Ok(headers.drain(..8).collect()))
            .map_err(|_| "headers [538, 545]")
            .unwrap();
        assert_syncing(&syncer, &store, &[1..=546], 546).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn stop_syncer() {
        let mut gen = ExtendedHeaderGenerator::new();
        let head = gen.next();

        let (syncer, _store, mut p2p_mock) = initialized_syncer(head.clone()).await;

        // network head height == 1, so nothing else is produced.
        p2p_mock.expect_no_cmd().await;

        syncer.stop();
        // Wait for Worker to stop
        sleep(Duration::from_millis(1)).await;
        assert!(matches!(
            syncer.info().await.unwrap_err(),
            SyncerError::WorkerDied
        ));
    }

    #[async_test]
    async fn all_peers_disconnected() {
        let mut gen = ExtendedHeaderGenerator::new();

        let _gap = gen.next_many(24);
        let header25 = gen.next();
        let _gap = gen.next_many(4);
        let header30 = gen.next();
        let _gap = gen.next_many(4);
        let header35 = gen.next();

        // Start Syncer and report height 30 as HEAD
        let (syncer, store, mut p2p_mock) = initialized_syncer(header30).await;

        // Wait for the request but do not reply to it.
        handle_session_batch(&mut p2p_mock, &[], 1..=29, false).await;

        p2p_mock.announce_all_peers_disconnected();
        // Syncer is now back to `connecting_event_loop`.
        p2p_mock.expect_no_cmd().await;

        // Accounce a non-trusted peer. Syncer in `connecting_event_loop` can progress only
        // if a trusted peer is connected.
        p2p_mock.announce_peer_connected();
        p2p_mock.expect_no_cmd().await;

        // Accounce a trusted peer.
        p2p_mock.announce_trusted_peer_connected();

        // Now syncer will send request for HEAD.
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);

        // Report an older head. Syncer should not accept it.
        respond_to.send(Ok(vec![header25])).unwrap();
        assert_syncing(&syncer, &store, &[30..=30], 30).await;

        // Syncer will request HEAD again after some time.
        sleep(Duration::from_secs(1)).await;
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);

        // Report newer HEAD than before.
        respond_to.send(Ok(vec![header35.clone()])).unwrap();
        assert_syncing(&syncer, &store, &[30..=30, 35..=35], 35).await;

        // Syncer initializes HeaderSub with the latest HEAD.
        let head_from_syncer = p2p_mock.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, header35);

        // Syncer now is in `connected_event_loop` and will try to sync the gap
        // that is closer to HEAD.
        let (height, amount, _respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 31);
        assert_eq!(amount, 4);

        p2p_mock.announce_all_peers_disconnected();
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn all_peers_disconnected_and_no_network_head_progress() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(30);

        let header30 = gen.next();

        // Start Syncer and report height 30 as HEAD
        let (syncer, store, mut p2p_mock) = initialized_syncer(header30.clone()).await;

        // Wait for the request but do not reply to it.
        handle_session_batch(&mut p2p_mock, &[], 1..=29, false).await;

        p2p_mock.announce_all_peers_disconnected();
        // Syncer is now back to `connecting_event_loop`.
        p2p_mock.expect_no_cmd().await;

        // Accounce a non-trusted peer. Syncer in `connecting_event_loop` can progress only
        // if a trusted peer is connected.
        p2p_mock.announce_peer_connected();
        p2p_mock.expect_no_cmd().await;

        // Accounce a trusted peer.
        p2p_mock.announce_trusted_peer_connected();

        // Now syncer will send request for HEAD.
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);

        // Report the same HEAD as before.
        respond_to.send(Ok(vec![header30.clone()])).unwrap();
        assert_syncing(&syncer, &store, &[30..=30], 30).await;

        // Syncer initializes HeaderSub with the latest HEAD.
        let head_from_syncer = p2p_mock.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, header30);

        // Syncer now is in `connected_event_loop` and will try to sync the gap
        handle_session_batch(&mut p2p_mock, &[], 1..=29, false).await;

        p2p_mock.announce_all_peers_disconnected();
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn non_contiguous_response() {
        let mut gen = ExtendedHeaderGenerator::new();
        let mut headers = gen.next_many(20);

        // Start Syncer and report last header as network head
        let (syncer, store, mut p2p_mock) = initialized_syncer(headers[19].clone()).await;

        let header10 = headers[10].clone();
        // make a gap in response, preserving the amount of headers returned
        headers[10] = headers[11].clone();

        // Syncer requests missing headers
        handle_session_batch(&mut p2p_mock, &headers, 1..=19, true).await;

        // Syncer should not apply headers from invalid response
        assert_syncing(&syncer, &store, &[20..=20], 20).await;

        // correct the response
        headers[10] = header10;

        // Syncer requests missing headers again
        handle_session_batch(&mut p2p_mock, &headers, 1..=19, true).await;

        // With a correct resposne, syncer should update the store
        assert_syncing(&syncer, &store, &[1..=20], 20).await;
    }

    #[async_test]
    async fn another_chain_response() {
        let headers = ExtendedHeaderGenerator::new().next_many(20);
        let headers_prime = ExtendedHeaderGenerator::new().next_many(20);

        // Start Syncer and report last header as network head
        let (syncer, store, mut p2p_mock) = initialized_syncer(headers[19].clone()).await;

        // Syncer requests missing headers
        handle_session_batch(&mut p2p_mock, &headers_prime, 1..=19, true).await;

        // Syncer should not apply headers from invalid response
        assert_syncing(&syncer, &store, &[20..=20], 20).await;

        // Syncer requests missing headers again
        handle_session_batch(&mut p2p_mock, &headers, 1..=19, true).await;

        // With a correct resposne, syncer should update the store
        assert_syncing(&syncer, &store, &[1..=20], 20).await;
    }

    async fn assert_syncing(
        syncer: &Syncer<InMemoryStore>,
        store: &InMemoryStore,
        expected_synced_ranges: &[RangeInclusive<u64>],
        expected_subjective_head: u64,
    ) {
        // Syncer receives responds on the same loop that receive other events.
        // Wait a bit to be processed.
        sleep(Duration::from_millis(1)).await;

        let store_ranges = store.get_stored_header_ranges().await.unwrap();
        let syncing_info = syncer.info().await.unwrap();

        assert_eq!(store_ranges.as_ref(), expected_synced_ranges);
        assert_eq!(syncing_info.stored_headers.as_ref(), expected_synced_ranges);
        assert_eq!(syncing_info.subjective_head, expected_subjective_head);
    }

    async fn initialized_syncer(
        head: ExtendedHeader,
    ) -> (Syncer<InMemoryStore>, Arc<InMemoryStore>, MockP2pHandle) {
        let events = EventChannel::new();
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());

        let syncer = Syncer::start(SyncerArgs {
            p2p: Arc::new(mock),
            store: store.clone(),
            event_pub: events.publisher(),
            batch_size: 512,
        })
        .unwrap();

        // Syncer is waiting for a trusted peer to connect
        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;
        handle.announce_trusted_peer_connected();

        // After connection is established Syncer asks current network HEAD
        let (height, amount, respond_to) = handle.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![head.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = handle.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, head);

        let head_height = head.height().value();
        assert_syncing(&syncer, &store, &[head_height..=head_height], head_height).await;

        (syncer, store, handle)
    }

    async fn handle_session_batch(
        p2p_mock: &mut MockP2pHandle,
        remaining_headers: &[ExtendedHeader],
        range: BlockRange,
        respond: bool,
    ) {
        range.validate().unwrap();

        let mut ranges_to_request = BlockRanges::new();
        ranges_to_request.insert_relaxed(&range).unwrap();

        for _ in 0..requests_in_session(range.len()) {
            let (height, amount, respond_to) =
                p2p_mock.expect_header_request_for_height_cmd().await;

            let requested_range = height..=height + amount - 1;
            ranges_to_request.remove_strict(requested_range);

            if respond {
                let header_index = remaining_headers
                    .iter()
                    .position(|h| h.height().value() == height)
                    .expect("height not found in provided headers");

                let response_range =
                    remaining_headers[header_index..header_index + amount as usize].to_vec();
                respond_to
                    .send(Ok(response_range))
                    .map_err(|_| format!("headers [{}, {}]", height, height + amount - 1))
                    .unwrap();
            }
        }

        assert!(
            ranges_to_request.is_empty(),
            "Some headers weren't requested. expected range: {}, not requested: {}",
            range.display(),
            ranges_to_request
        );
    }

    fn requests_in_session(headers: u64) -> usize {
        let max_requests = headers.div_ceil(header_session::MAX_AMOUNT_PER_REQ) as usize;
        let min_requests = headers.div_ceil(header_session::MIN_AMOUNT_PER_REQ) as usize;

        if max_requests > header_session::MAX_CONCURRENT_REQS {
            // if we have to do more requests than our concurrency limit anyway
            max_requests
        } else {
            // otherwise we can handle batch fully concurrent
            header_session::MAX_CONCURRENT_REQS.min(min_requests)
        }
    }

    impl BlockRanges {
        fn remove_strict(&mut self, range: BlockRange) {
            for stored in self.as_ref() {
                if stored.contains(range.start()) && stored.contains(range.end()) {
                    self.remove_relaxed(range).unwrap();
                    return;
                }
            }

            panic!("block ranges ({self}) don't contain {}", range.display());
        }
    }
}
