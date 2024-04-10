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
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::select;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, instrument, warn, Instrument};

use crate::executor::{sleep, spawn, spawn_cancellable, Interval};
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};
use crate::utils::OneshotSenderExt;

type Result<T, E = SyncerError> = std::result::Result<T, E>;

const MAX_HEADERS_IN_BATCH: u64 = 512;
const TRY_INIT_BACKOFF_MAX_INTERVAL: Duration = Duration::from_secs(60);

/// Representation of all the errors that can occur when interacting with the [`Syncer`].
#[derive(Debug, thiserror::Error)]
pub enum SyncerError {
    /// An error propagated from the [`P2p`] module.
    #[error(transparent)]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Store`] module.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// An error propagated from the [`celestia_types`].
    #[error(transparent)]
    Celestia(#[from] celestia_types::Error),

    /// The worker has died.
    #[error("Worker died")]
    WorkerDied,

    /// Channel has been closed unexpectedly.
    #[error("Channel closed unexpectedly")]
    ChannelClosedUnexpectedly,
}

impl From<oneshot::error::RecvError> for SyncerError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        SyncerError::ChannelClosedUnexpectedly
    }
}

/// Component responsible for synchronizing block headers from the network.
#[derive(Debug)]
pub struct Syncer<S>
where
    S: Store + 'static,
{
    cmd_tx: mpsc::Sender<SyncerCmd>,
    cancellation_token: CancellationToken,
    _store: PhantomData<S>,
}

/// Arguments used to configure the [`Syncer`].
pub struct SyncerArgs<S>
where
    S: Store + 'static,
{
    /// Hash of the genesis block.
    pub genesis_hash: Option<Hash>,
    /// Handler for the peer to peer messaging.
    pub p2p: Arc<P2p>,
    /// Headers storage.
    pub store: Arc<S>,
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
    /// The height the [`Syncer`] is currently synchronized to.
    pub local_head: u64,
    /// Syncing target. The latest height seen in the network that was successfully verified.
    pub subjective_head: u64,
}

impl<S> Syncer<S>
where
    S: Store,
{
    /// Create and start the [`Syncer`].
    pub fn start(args: SyncerArgs<S>) -> Result<Self> {
        let cancellation_token = CancellationToken::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut worker = Worker::new(args, cancellation_token.child_token(), cmd_rx)?;

        spawn(async move {
            worker.run().await;
        });

        Ok(Syncer {
            cancellation_token,
            cmd_tx,
            _store: PhantomData,
        })
    }

    /// Stop the [`Syncer`].
    pub fn stop(&self) {
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
    pub async fn info(&self) -> Result<SyncingInfo> {
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
    p2p: Arc<P2p>,
    store: Arc<S>,
    header_sub_watcher: watch::Receiver<Option<ExtendedHeader>>,
    genesis_hash: Option<Hash>,
    subjective_head_height: Option<u64>,
    headers_tx: mpsc::Sender<Result<Vec<ExtendedHeader>, P2pError>>,
    headers_rx: mpsc::Receiver<Result<Vec<ExtendedHeader>, P2pError>>,
    ongoing_batch: Option<Ongoing>,
}

struct Ongoing {
    start: u64,
    end: u64,
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
        let header_sub_watcher = args.p2p.header_sub_watcher();
        let (headers_tx, headers_rx) = mpsc::channel(1);

        Ok(Worker {
            cancellation_token,
            cmd_rx,
            p2p: args.p2p,
            store: args.store,
            header_sub_watcher,
            genesis_hash: args.genesis_hash,
            subjective_head_height: None,
            headers_tx,
            headers_rx,
            ongoing_batch: None,
        })
    }

    async fn run(&mut self) {
        loop {
            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connecting_event_loop().await;

            if self.cancellation_token.is_cancelled() {
                break;
            }

            self.connected_event_loop().await;
        }

        debug!("Syncer stopped");
    }

    /// The responsibility of this event loop is to await a trusted peer to
    /// connect and get the network head, while accepting commands.
    async fn connecting_event_loop(&mut self) {
        debug!("Entering connecting_event_loop");

        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
        let mut try_init_result = self.spawn_try_init().fuse();

        self.report().await;

        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    break;
                }
                _ = report_interval.tick() => {
                    self.report().await;
                }
                Ok(network_head_height) = &mut try_init_result => {
                    info!("Setting initial subjective head to {network_head_height}");
                    self.subjective_head_height = Some(network_head_height);
                    break;
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    self.on_cmd(cmd).await;
                }
            }
        }
    }

    /// The reponsibility of this event loop is to start the syncing process,
    /// handles events from HeaderSub, and accept commands.
    async fn connected_event_loop(&mut self) {
        debug!("Entering connected_event_loop");

        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
        let mut peer_tracker_info_watcher = self.p2p.peer_tracker_info_watcher();

        // Check if connection status changed before creating the watcher
        if peer_tracker_info_watcher.borrow().num_connected_peers == 0 {
            warn!("All peers disconnected");
            return;
        }

        self.fetch_next_batch().await;
        self.report().await;

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
                    self.report().await;
                }
                _ = self.header_sub_watcher.changed() => {
                    self.on_header_sub_message().await;
                    self.fetch_next_batch().await;
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    self.on_cmd(cmd).await;
                }
                Some(res) = self.headers_rx.recv() => {
                    self.on_fetch_next_batch_result(res).await;
                    self.fetch_next_batch().await;
                }
            }
        }

        if let Some(ongoing) = self.ongoing_batch.take() {
            warn!(
                "Cancelling fetching of [{}, {}]",
                ongoing.start, ongoing.end
            );
            ongoing.cancellation_token.cancel();
        }
    }

    async fn syncing_info(&self) -> SyncingInfo {
        SyncingInfo {
            local_head: self.store.head_height().await.unwrap_or(0),
            subjective_head: self.subjective_head_height.unwrap_or(0),
        }
    }

    #[instrument(skip_all)]
    async fn report(&mut self) {
        let SyncingInfo {
            local_head,
            subjective_head,
        } = self.syncing_info().await;

        let ongoing_batch = self
            .ongoing_batch
            .as_ref()
            .map(|ongoing| format!("[{}, {}]", ongoing.start, ongoing.end))
            .unwrap_or_else(|| "None".to_string());

        info!("syncing: {local_head}/{subjective_head}, ongoing batch: {ongoing_batch}",);
    }

    fn spawn_try_init(&self) -> oneshot::Receiver<u64> {
        let p2p = self.p2p.clone();
        let store = self.store.clone();
        let genesis_hash = self.genesis_hash;
        let (tx, rx) = oneshot::channel();

        let fut = async move {
            let mut backoff = ExponentialBackoffBuilder::default()
                .with_max_interval(TRY_INIT_BACKOFF_MAX_INTERVAL)
                .with_max_elapsed_time(None)
                .build();

            loop {
                match try_init(&p2p, &*store, genesis_hash).await {
                    Ok(network_height) => {
                        tx.maybe_send(network_height);
                        break;
                    }
                    Err(e) => {
                        let sleep_dur = backoff
                            .next_backoff()
                            .expect("backoff never stops retrying");

                        warn!("Intialization of subjective head failed: {e}. Trying again in {sleep_dur:?}.");
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

    async fn on_cmd(&mut self, cmd: SyncerCmd) {
        match cmd {
            SyncerCmd::GetInfo { respond_to } => {
                let info = self.syncing_info().await;
                respond_to.maybe_send(info);
            }
        }
    }

    #[instrument(skip_all)]
    async fn on_header_sub_message(&mut self) {
        // If subjective head isn't set, do nothing.
        // We do this to avoid some edge cases.
        if self.subjective_head_height.is_none() {
            return;
        }

        let Some(new_head) = self.header_sub_watcher.borrow().to_owned() else {
            // Nothing to do
            return;
        };

        let new_head_height = new_head.height().value();

        // We don't want to interfere with any ongoing batch fetching
        if self.ongoing_batch.is_none() {
            if let Ok(store_head_height) = self.store.head_height().await {
                // If our new header is adjacent to the HEAD of the store
                if store_head_height + 1 == new_head_height {
                    // Header is already verified by HeaderSub
                    if self.store.append_single_unchecked(new_head).await.is_ok() {
                        info!("Added header {new_head_height} from HeaderSub");
                    }
                }
            }
        }

        self.subjective_head_height = Some(new_head_height);
    }

    #[instrument(skip_all)]
    async fn fetch_next_batch(&mut self) {
        if self.ongoing_batch.is_some() {
            // Another batch is ongoing. We do not parallelize `Syncer`
            // by design. Any parallel requests are done in the
            // HeaderEx client through `Session`.
            //
            // Nothing to schedule
            return;
        }

        let Some(subjective_head_height) = self.subjective_head_height else {
            // Nothing to schedule
            return;
        };

        let Ok(local_head) = self.store.get_head().await else {
            // Nothing to schedule
            return;
        };

        let amount = subjective_head_height
            .saturating_sub(local_head.height().value())
            .min(MAX_HEADERS_IN_BATCH);

        if amount == 0 {
            // Nothing to schedule
            return;
        }

        if self.p2p.peer_tracker_info().num_connected_peers == 0 {
            // No connected peers. We can't do the request.
            // This will be recovered by `run`.
            return;
        }

        let start = local_head.height().value() + 1;
        let end = start + amount - 1;
        let cancellation_token = self.cancellation_token.child_token();

        self.ongoing_batch = Some(Ongoing {
            start,
            end,
            cancellation_token: cancellation_token.clone(),
        });
        info!("Fetching batch {start} until {end}");

        let tx = self.headers_tx.clone();
        let p2p = self.p2p.clone();

        spawn_cancellable(cancellation_token, async move {
            let res = p2p.get_verified_headers_range(&local_head, amount).await;
            let _ = tx.send(res).await;
        });
    }

    #[instrument(skip_all)]
    async fn on_fetch_next_batch_result(&mut self, res: Result<Vec<ExtendedHeader>, P2pError>) {
        let Some(ongoing) = self.ongoing_batch.take() else {
            warn!("No batch was scheduled, however result was received. Discarding it.");
            return;
        };

        let start = ongoing.start;
        let end = ongoing.end;

        let headers = match res {
            Ok(headers) => headers,
            Err(e) => {
                warn!("Failed to receive batch {start} until {end}: {e}");
                return;
            }
        };

        // Headers are already verified by `get_verified_headers_range`,
        // so `append_unchecked` is used for optimization.
        if let Err(e) = self.store.append_unchecked(headers).await {
            warn!("Failed to store batch {start} until {end}: {e}");
        }
    }
}

async fn try_init<S>(p2p: &P2p, store: &S, genesis_hash: Option<Hash>) -> Result<u64>
where
    S: Store,
{
    p2p.wait_connected_trusted().await?;

    // IF store is empty, intialize it with genesis
    if store.head_height().await.is_err() {
        let genesis = match genesis_hash {
            Some(hash) => p2p.get_header(hash).await?,
            None => {
                warn!("Genesis hash is not set, requesting height 1.");
                p2p.get_header_by_height(1).await?
            }
        };

        // Store genesis
        store.append_single_unchecked(genesis).await?;
    }

    let network_head = p2p.get_head_header().await?;
    let network_head_height = network_head.height().value();

    p2p.init_header_sub(network_head).await?;

    Ok(network_head_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, gen_filled_store, MockP2pHandle};
    use celestia_types::test_utils::ExtendedHeaderGenerator;

    #[async_test]
    async fn init_without_genesis_hash() {
        let (mock, mut handle) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let genesis = gen.next();

        let _syncer = Syncer::start(SyncerArgs {
            genesis_hash: None,
            p2p: Arc::new(mock),
            store: Arc::new(InMemoryStore::new()),
        })
        .unwrap();

        // Syncer is waiting for a trusted peer to connect
        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;
        handle.announce_trusted_peer_connected();

        // After connection is established Syncer asks for genesis header
        let (height, amount, respond_to) = handle.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![genesis.clone()])).unwrap();

        // After genesis header it asks for the current HEAD
        let (height, amount, respond_to) = handle.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![genesis.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = handle.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, genesis);

        // Genesis == HEAD, so nothing else is produced.
        handle.expect_no_cmd().await;
    }

    #[async_test]
    async fn init_with_genesis_hash() {
        let mut gen = ExtendedHeaderGenerator::new();
        let genesis = gen.next();

        let (_syncer, _store, mut p2p_mock) =
            initialized_syncer(genesis.clone(), genesis.clone()).await;

        // Genesis == HEAD, so nothing else is produced.
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn syncing() {
        let mut gen = ExtendedHeaderGenerator::new();
        let genesis = gen.next();
        let headers_2_26 = gen.next_many(25);

        let (syncer, store, mut p2p_mock) =
            initialized_syncer(genesis.clone(), headers_2_26[24].clone()).await;

        // Expecting request for [2, 26]
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 2);
        assert_eq!(amount, 25);
        // Respond to syncer
        respond_to
            .send(Ok(headers_2_26))
            // Mapping to avoid spamming error message on failure
            .map_err(|_| "headers [2, 26]")
            .unwrap();
        assert_syncing(&syncer, &store, 26, 26).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;

        // New HEAD was received by HeaderSub (height 27)
        let header27 = gen.next();
        p2p_mock.announce_new_head(header27.clone());
        // Height 27 is adjacent to the last header of Store, so it is appended
        // immediately
        assert_syncing(&syncer, &store, 27, 27).await;
        p2p_mock.expect_no_cmd().await;

        // New HEAD was received by HeaderSub (height 30)
        let header_28_30 = gen.next_many(3);
        p2p_mock.announce_new_head(header_28_30[2].clone());
        assert_syncing(&syncer, &store, 27, 30).await;

        // New HEAD is not adjacent to store, so Syncer requests a range
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 28);
        assert_eq!(amount, 3);
        respond_to
            .send(Ok(header_28_30))
            .map_err(|_| "headers [28, 30]")
            .unwrap();
        assert_syncing(&syncer, &store, 30, 30).await;

        // New HEAD was received by HeaderSub (height 1058)
        let mut headers = gen.next_many(1028);
        p2p_mock.announce_new_head(headers.last().cloned().unwrap());
        assert_syncing(&syncer, &store, 30, 1058).await;

        // Syncer requested the first batch ([31, 542])
        handle_session_batch(
            &mut p2p_mock,
            &mut headers,
            vec![
                (31, 64),
                (95, 64),
                (159, 64),
                (223, 64),
                (287, 64),
                (351, 64),
                (415, 64),
                (479, 64),
            ],
        )
        .await;
        assert_syncing(&syncer, &store, 542, 1058).await;

        // Still syncing, but new HEAD arrived (height 1059)
        headers.push(gen.next());
        p2p_mock.announce_new_head(headers.last().cloned().unwrap());
        assert_syncing(&syncer, &store, 542, 1059).await;

        // Syncer requested the second batch ([543, 1054])
        handle_session_batch(
            &mut p2p_mock,
            &mut headers,
            vec![
                (543, 64),
                (607, 64),
                (671, 64),
                (735, 64),
                (799, 64),
                (863, 64),
                (927, 64),
                (991, 64),
            ],
        )
        .await;
        assert_syncing(&syncer, &store, 1054, 1059).await;

        // Syncer requested the last batch ([1055, 1059])
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1055);
        assert_eq!(amount, 5);
        respond_to
            .send(Ok(headers.drain(..5).collect()))
            .map_err(|_| "headers [1055, 1059]")
            .unwrap();
        assert_syncing(&syncer, &store, 1059, 1059).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn start_with_filled_store() {
        let (p2p, mut p2p_mock) = P2p::mocked();
        let (store, mut gen) = gen_filled_store(25);
        let store = Arc::new(store);

        let genesis = store.get_by_height(1).await.unwrap();
        let mut headers = gen.next_many(520);
        let network_head = headers.last().cloned().unwrap();

        let syncer = Syncer::start(SyncerArgs {
            genesis_hash: Some(genesis.hash()),
            p2p: Arc::new(p2p),
            store: store.clone(),
        })
        .unwrap();

        p2p_mock.announce_trusted_peer_connected();

        // Store already has genesis, so Syncer asks only for current HEAD
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![network_head.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = p2p_mock.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, network_head);

        assert_syncing(&syncer, &store, 25, 545).await;

        // Syncer requested the first batch ([26, 537])
        handle_session_batch(
            &mut p2p_mock,
            &mut headers,
            vec![
                (26, 64),
                (90, 64),
                (154, 64),
                (218, 64),
                (282, 64),
                (346, 64),
                (410, 64),
                (474, 64),
            ],
        )
        .await;
        assert_syncing(&syncer, &store, 537, 545).await;

        // Syncer requested the last batch ([538, 545])
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 538);
        assert_eq!(amount, 8);
        respond_to
            .send(Ok(headers.drain(..8).collect()))
            .map_err(|_| "headers [538, 545]")
            .unwrap();
        assert_syncing(&syncer, &store, 545, 545).await;

        // Syncer is fulling synced and awaiting for events
        p2p_mock.expect_no_cmd().await;
    }

    #[async_test]
    async fn stop_syncer() {
        let mut gen = ExtendedHeaderGenerator::new();
        let genesis = gen.next();

        let (syncer, _store, mut p2p_mock) =
            initialized_syncer(genesis.clone(), genesis.clone()).await;

        // Genesis == HEAD, so nothing else is produced.
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
        let genesis = gen.next();
        let headers_2_27 = gen.next_many(26);

        // Start Syncer and report height 25 as HEAD
        let (syncer, store, mut p2p_mock) =
            initialized_syncer(genesis.clone(), headers_2_27[24].clone()).await;

        // Wait for the request but do not reply to it
        let (height, amount, _respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 2);
        assert_eq!(amount, 25);

        p2p_mock.announce_all_peers_disconnected();
        p2p_mock.expect_no_cmd().await;

        p2p_mock.announce_trusted_peer_connected();

        // Syncer is now back to `connecting_event_loop`, so we expect a request for HEAD
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        // Now HEAD is height 27
        respond_to.send(Ok(vec![headers_2_27[25].clone()])).unwrap();

        // Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = p2p_mock.expect_init_header_sub().await;
        assert_eq!(&head_from_syncer, &headers_2_27[25]);

        // Syncer now moved to `connected_event_loop`
        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 2);
        assert_eq!(amount, 26);
        respond_to
            .send(Ok(headers_2_27))
            // Mapping to avoid spamming error message on failure
            .map_err(|_| "headers [2, 27]")
            .unwrap();
        assert_syncing(&syncer, &store, 27, 27).await;

        // Node is fully synced, so nothing else is produced.
        p2p_mock.expect_no_cmd().await;
    }

    async fn assert_syncing(
        syncer: &Syncer<InMemoryStore>,
        store: &InMemoryStore,
        expected_local_head: u64,
        expected_subjective_head: u64,
    ) {
        // Syncer receives responds on the same loop that receive other events.
        // Wait a bit to be processed.
        sleep(Duration::from_millis(1)).await;

        let store_height = store.head_height().await.unwrap();
        let syncing_info = syncer.info().await.unwrap();

        assert_eq!(store_height, expected_local_head);
        assert_eq!(syncing_info.local_head, expected_local_head);
        assert_eq!(syncing_info.subjective_head, expected_subjective_head);
    }

    async fn initialized_syncer(
        genesis: ExtendedHeader,
        head: ExtendedHeader,
    ) -> (Syncer<InMemoryStore>, Arc<InMemoryStore>, MockP2pHandle) {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());

        let syncer = Syncer::start(SyncerArgs {
            genesis_hash: Some(genesis.hash()),
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        // Syncer is waiting for a trusted peer to connect
        handle.expect_no_cmd().await;
        handle.announce_peer_connected();
        handle.expect_no_cmd().await;
        handle.announce_trusted_peer_connected();

        // After connection is established Syncer asks for genesis header
        let (hash, respond_to) = handle.expect_header_request_for_hash_cmd().await;
        assert_eq!(hash, genesis.hash());
        respond_to.send(Ok(vec![genesis])).unwrap();

        // After genesis header it asks for the current HEAD
        let (height, amount, respond_to) = handle.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 0);
        assert_eq!(amount, 1);
        respond_to.send(Ok(vec![head.clone()])).unwrap();

        // Now Syncer initializes HeaderSub with the latest HEAD
        let head_from_syncer = handle.expect_init_header_sub().await;
        assert_eq!(head_from_syncer, head);

        (syncer, store, handle)
    }

    async fn handle_session_batch(
        p2p_mock: &mut MockP2pHandle,
        remaining_headers: &mut Vec<ExtendedHeader>,
        mut requests: Vec<(u64, u64)>,
    ) {
        let first_start = requests[0].0;
        let mut sum_amount = 0;

        for _ in 0..requests.len() {
            let (height, amount, respond_to) =
                p2p_mock.expect_header_request_for_height_cmd().await;

            let pos = requests
                .iter()
                .position(|x| *x == (height, amount))
                .expect("invalid request");
            requests.remove(pos);

            let start = (height - first_start) as usize;
            let end = (height - first_start + amount) as usize;
            respond_to
                .send(Ok(remaining_headers[start..end].to_vec()))
                .map_err(|_| format!("headers [{}, {}]", height, height + amount - 1))
                .unwrap();

            sum_amount += amount;
        }

        // Remove already sent batch
        remaining_headers.drain(..sum_amount as usize);
    }
}
