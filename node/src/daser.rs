//! Component responsible for data availability sampling of the already synchronized block
//! headers announced on the Celestia network.
//!
//! When a new header is inserted into the [`Store`], [`Daser`] gets notified. It then fetches
//! random [`Sample`]s of the block via Shwap protocol and verifies them. If all the samples
//! get verified successfuly, then block is marked as accepted. Otherwise, if [`Daser`] doesn't
//! receive valid samples, block is marked as not accepted and data sampling continues.
//!
//! [`Sample`]: celestia_types::sample::Sample

use std::collections::HashSet;
use std::sync::Arc;

use celestia_types::ExtendedHeader;
use cid::Cid;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use instant::Instant;
use rand::Rng;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error};

use crate::executor::spawn;
use crate::p2p::shwap::convert_cid;
use crate::p2p::{P2p, P2pError};
use crate::store::{Store, StoreError};

const MAX_SAMPLES_NEEDED: usize = 16;

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
            max_samples_needed: MAX_SAMPLES_NEEDED,
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
        let now = Instant::now();
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
                // Validation is done in Bitswap level thought `ShwapMultihasher`.
                // When a sample is not valid, then is will never be delivered
                // to us as the data of the CID. Because of that, the only way
                // to know that data sampling verification failed is when a query
                // times out.
                Err(P2pError::BitswapQueryTimeout) => accepted = false,
                Err(e) => return Err(e.into()),
            }
        }

        debug!(
            "Data sampling of {} is {}. Took {:?}",
            header.height(),
            if accepted { "accepted" } else { "rejected" },
            now.elapsed()
        );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, dah_of_eds, generate_fake_eds, MockP2pHandle};
    use celestia_tendermint_proto::Protobuf;
    use celestia_types::sample::{Sample, SampleId};
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::{AxisType, ExtendedDataSquare};

    #[async_test]
    async fn received_valid_samples() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());

        let _daser = Daser::start(DaserArgs {
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;

        gen_and_sample_block(&mut handle, &mut gen, &store, 2, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, 4, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, 8, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, 16, false).await;
    }

    #[async_test]
    async fn received_invalid_sample() {
        let (mock, mut handle) = P2p::mocked();
        let store = Arc::new(InMemoryStore::new());

        let _daser = Daser::start(DaserArgs {
            p2p: Arc::new(mock),
            store: store.clone(),
        })
        .unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        handle.expect_no_cmd().await;

        gen_and_sample_block(&mut handle, &mut gen, &store, 2, false).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, 4, true).await;
        gen_and_sample_block(&mut handle, &mut gen, &store, 8, false).await;
    }

    async fn gen_and_sample_block(
        handle: &mut MockP2pHandle,
        gen: &mut ExtendedHeaderGenerator,
        store: &InMemoryStore,
        square_len: usize,
        simulate_invalid_sampling: bool,
    ) {
        let eds = generate_fake_eds(square_len);
        let dah = dah_of_eds(&eds);
        let header = gen.next_with_dah(dah);
        let height = header.height().value();

        store.append_single(header).await.unwrap();

        let mut cids = Vec::new();

        for i in 0..(square_len * square_len).min(MAX_SAMPLES_NEEDED) {
            let (cid, respond_to) = handle.expect_get_shwap_cid().await;

            // Simulate invalid sample by triggering BitswapQueryTimeout
            if simulate_invalid_sampling && i == 2 {
                respond_to.send(Err(P2pError::BitswapQueryTimeout)).unwrap();
                continue;
            }

            let sample_id: SampleId = cid.try_into().unwrap();
            assert_eq!(sample_id.row.block_height, height);

            let sample = gen_sample_of_cid(sample_id, &eds, store).await;
            let sample_bytes = sample.encode_vec().unwrap();

            respond_to.send(Ok(sample_bytes)).unwrap();
            cids.push(cid);
        }

        handle.expect_no_cmd().await;

        let sampling_metadata = store.get_sampling_metadata(height).await.unwrap().unwrap();
        assert_eq!(sampling_metadata.accepted, !simulate_invalid_sampling);
        assert_eq!(sampling_metadata.cids_sampled, cids);
    }

    async fn gen_sample_of_cid(
        sample_id: SampleId,
        eds: &ExtendedDataSquare,
        store: &InMemoryStore,
    ) -> Sample {
        let header = store
            .get_by_height(sample_id.row.block_height)
            .await
            .unwrap();

        let row = sample_id.row.index as usize;
        let col = sample_id.index as usize;
        let index = row * header.dah.square_len() + col;

        Sample::new(AxisType::Row, index, eds, header.height().value()).unwrap()
    }
}
