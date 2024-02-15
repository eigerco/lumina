//! Component responsible for data availability sampling of the already synchronized block
//! headers announced in the Celestia network.
//!
//! When a new header is insert in the [`Store`], [`Daser`] gets informed, then it fetches
//! random [`Sample`]s of the block via Shwap protocol and verifies them. If all samples
//! get verified successfuly, then block is marked as accepted.
//!
//! [`Sample`]: celestia_types::sample::Sample

use std::collections::HashSet;
use std::sync::Arc;

use celestia_types::ExtendedHeader;
use cid::Cid;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use rand::Rng;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::executor::spawn;
use crate::p2p::shwap::convert_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};

const DEFAULT_MAX_SAMPLES_NEEDED: usize = 16;

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
}

impl Daser {
    /// Create and start the [`Daser`].
    pub fn start<S>(args: DaserArgs<S>) -> Result<Self>
    where
        S: Store + 'static,
    {
        let cancellation_token = CancellationToken::new();
        let mut worker = Worker::new(args, cancellation_token.child_token())?;

        spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Fatal DASer error: {e}");
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
    p2p: Arc<P2p>,
    store: Arc<S>,
    max_samples_needed: usize,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(args: DaserArgs<S>, cancellation_token: CancellationToken) -> Result<Worker<S>> {
        Ok(Worker {
            cancellation_token,
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
        let (sampled_cids, accepted) = self.sample_block(&header).await?;

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
