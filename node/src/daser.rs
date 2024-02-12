use std::collections::HashSet;
use std::sync::Arc;

use celestia_types::ExtendedHeader;
use cid::Cid;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use rand::Rng;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::spawn;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::p2p::shwap::convert_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};

const DEFAULT_MAX_SAMPLES_NEEDED: usize = 16;

type Result<T, E = DaserError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum DaserError {
    #[error(transparent)]
    P2p(#[from] P2pError),

    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct Daser {
    cmd_tx: mpsc::Sender<DaserCmd>,
    cancellation_token: CancellationToken,
}

pub struct DaserArgs<S>
where
    S: Store + 'static,
{
    pub p2p: Arc<P2p<S>>,
    pub store: Arc<S>,
}

#[derive(Debug)]
enum DaserCmd {}

impl Daser {
    pub fn start<S>(args: DaserArgs<S>) -> Result<Self>
    where
        S: Store,
    {
        let cancellation_token = CancellationToken::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut worker = Worker::new(args, cancellation_token.child_token(), cmd_rx)?;

        spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Fatal DASer error: {e}");
            }
        });

        Ok(Daser {
            cancellation_token,
            cmd_tx,
        })
    }

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
    cmd_rx: mpsc::Receiver<DaserCmd>,
    p2p: Arc<P2p<S>>,
    store: Arc<S>,
    max_samples_needed: usize,
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
            cancellation_token,
            cmd_rx,
            p2p: args.p2p,
            store: args.store,
            max_samples_needed: DEFAULT_MAX_SAMPLES_NEEDED,
        })
    }

    async fn run(&mut self) -> Result<()> {
        let cancellation_token = self.cancellation_token.clone();

        loop {
            select! {
                _ = cancellation_token.cancelled() => break,
                res = self.sample_next_block() => res?,
            }
        }

        Ok(())
    }

    async fn sample_next_block(&mut self) -> Result<()> {
        let height = self.store.next_unsampled_height().await?;
        self.store.wait_height(height).await?;

        let header = self.store.get_by_height(height).await?;
        let (sampled_cids, accepted) = dbg!(self.sample_block(&header).await?);

        self.store
            .update_sampling_metadata(height, accepted, sampled_cids)
            .await?;

        Ok(())
    }

    async fn sample_block(&mut self, header: &ExtendedHeader) -> Result<(Vec<Cid>, bool)> {
        let block_len = header.dah.square_len() * header.dah.square_len();
        let indexes = random_indexes(block_len, self.max_samples_needed);
        let mut futs = FuturesUnordered::new();

        for index in indexes {
            let fut = self
                .p2p
                .get_sample(index, header.dah.square_len(), header.height().value());
            futs.push(fut);
        }

        let mut cids: Vec<Cid> = Vec::new();
        let mut accepted = true;

        while let Some(res) = futs.next().await {
            match res {
                Ok(sample) => cids.push(convert_cid(&sample.sample_id.into())?),
                Err(P2pError::BitswapQueryTimeout) => accepted = false,
                Err(e) => return Err(e.into()),
            }
        }

        Ok((cids, accepted))
    }
}

fn random_indexes(block_len: usize, max_samples_needed: usize) -> HashSet<usize> {
    // If block length is smaller than `max_samples_needed`, we are going
    // to sample the whole block. Randomation is not needed for this.
    if block_len <= max_samples_needed {
        return (0..block_len).collect();
    }

    let mut indexes = HashSet::with_capacity(max_samples_needed);
    let mut rng = rand::thread_rng();

    while indexes.len() < max_samples_needed {
        indexes.insert(rng.gen::<usize>() % block_len);
    }

    indexes
}
