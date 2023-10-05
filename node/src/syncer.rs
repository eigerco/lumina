use std::marker::PhantomData;
use std::sync::Arc;

use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use tokio::select;
use tokio::sync::{mpsc, watch};
use tracing::{info, instrument, warn};

use crate::executor::spawn;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};

type Result<T, E = SyncerError> = std::result::Result<T, E>;

const MAX_HEADERS_PER_TIME: u64 = 512;

#[derive(Debug, thiserror::Error)]
pub enum SyncerError {
    #[error(transparent)]
    P2p(#[from] P2pError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Celestia(#[from] celestia_types::Error),

    #[error("Worker died")]
    WorkerDied,
}

#[allow(unused)]
#[derive(Debug)]
pub struct Syncer<S>
where
    S: Store + 'static,
{
    cmd_tx: mpsc::Sender<SyncerCmd>,
    _store: PhantomData<S>,
}

pub struct SyncerArgs<S>
where
    S: Store + 'static,
{
    pub genesis_hash: Option<Hash>,
    pub p2p: Arc<P2p<S>>,
    pub store: Arc<S>,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum SyncerCmd {}

impl<S> Syncer<S>
where
    S: Store,
{
    pub async fn start(args: SyncerArgs<S>) -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut worker = Worker::new(args, cmd_rx)?;

        spawn(async move {
            worker.run().await;
        });

        Ok(Syncer {
            cmd_tx,
            _store: PhantomData,
        })
    }

    pub async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    pub async fn send_command(&self, cmd: SyncerCmd) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| SyncerError::WorkerDied)
    }
}

struct Worker<S>
where
    S: Store + 'static,
{
    cmd_rx: mpsc::Receiver<SyncerCmd>,
    p2p: Arc<P2p<S>>,
    store: Arc<S>,
    header_sub_watcher: watch::Receiver<Option<ExtendedHeader>>,
    genesis_hash: Option<Hash>,
    subjective_head_height: Option<u64>,
    response_tx: mpsc::Sender<Result<Vec<ExtendedHeader>, P2pError>>,
    response_rx: mpsc::Receiver<Result<Vec<ExtendedHeader>, P2pError>>,
    ongoing_batch: Option<(u64, u64)>,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(args: SyncerArgs<S>, cmd_rx: mpsc::Receiver<SyncerCmd>) -> Result<Self> {
        let header_sub_watcher = args.p2p.header_sub_watcher();
        let (response_tx, response_rx) = mpsc::channel(1);

        Ok(Worker {
            cmd_rx,
            p2p: args.p2p,
            store: args.store,
            header_sub_watcher,
            genesis_hash: args.genesis_hash,
            subjective_head_height: None,
            response_tx,
            response_rx,
            ongoing_batch: None,
        })
    }

    async fn run(&mut self) {
        while let Err(e) = self.try_init().await {
            warn!("Intialization of subjective head failed: {e}. Trying again.");
        }

        self.fetch_next_batch().await;

        loop {
            select! {
                _ = self.header_sub_watcher.changed() => {
                    self.on_header_sub_message().await;
                    self.fetch_next_batch().await;
                }
                Some(_cmd) = self.cmd_rx.recv() => {
                    // TODO
                }
                Some(res) = self.response_rx.recv() => {
                    self.on_fetch_next_batch_result(res).await;
                    self.fetch_next_batch().await;
                }
            }
        }
    }

    #[instrument(skip_all)]
    async fn try_init(&mut self) -> Result<()> {
        self.p2p.wait_connected_trusted().await?;

        // IF store is empty, intialize it with genesis
        if self.store.head_height().await.is_err() {
            let genesis = match self.genesis_hash {
                Some(hash) => self.p2p.get_header(hash).await?,
                None => {
                    warn!("Genesis hash is not set, requesting height 1.");
                    self.p2p.get_header_by_height(1).await?
                }
            };

            self.store.append_single_unchecked(genesis).await?;
        }

        let network_head = self.p2p.get_head_header().await?;
        let network_head_height = network_head.height().value();

        self.subjective_head_height = Some(network_head_height);
        self.p2p.init_header_sub(network_head).await?;

        info!("Setting initial subjective head to {network_head_height}");

        Ok(())
    }

    #[instrument(skip_all)]
    async fn on_header_sub_message(&mut self) {
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
                        info!("Added {new_head_height} from HeaderSub");
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
            // Exchange client through `Session`.
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
            .min(MAX_HEADERS_PER_TIME);

        if amount == 0 {
            // Nothing to schedule
            return;
        }

        let start = local_head.height().value() + 1;
        let end = start + amount - 1;

        self.ongoing_batch = Some((start, end));
        info!("Fetching batch {start} until {end}");

        let tx = self.response_tx.clone();
        let p2p = self.p2p.clone();

        spawn(async move {
            let res = p2p.get_verified_headers_range(&local_head, amount).await;
            let _ = tx.send(res).await;
        });
    }

    #[instrument(skip_all)]
    async fn on_fetch_next_batch_result(&mut self, res: Result<Vec<ExtendedHeader>, P2pError>) {
        let Some((start, end)) = self.ongoing_batch.take() else {
            warn!("No batch was scheduled, however result was received. Discarding it.");
            return;
        };

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
