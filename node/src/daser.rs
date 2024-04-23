//! Component responsible for data availability sampling of the already synchronized block
//! headers announced on the Celestia network.
//!
//! When a new header is inserted into the [`Store`], [`Daser`] gets notified. It then fetches
//! random [`Sample`]s of the block via Shwap protocol and verifies them. If all the samples
//! get verified successfuly, then block is marked as accepted. Otherwise, if [`Daser`] doesn't
//! receive valid samples, block is marked as not accepted and data sampling continues.
//!
//! [`Sample`]: celestia_types::sample::Sample

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
use tracing::{debug, error};

use crate::executor::spawn;
use crate::p2p::shwap::sample_cid;
use crate::p2p::{P2p, P2pError, GET_SAMPLE_TIMEOUT};
use crate::store::{SamplingStatus, Store, StoreError};
use crate::utils::FusedReusableBoxFuture;

const MAX_SAMPLES_NEEDED: usize = 16;
const SAMPLING_WINDOW: Duration = Duration::from_secs(14678036);

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
            p2p: args.p2p,
            store: args.store,
            max_samples_needed: MAX_SAMPLES_NEEDED,
            sampling_fut: FusedReusableBoxFuture::terminated(),
            queue: VecDeque::new(),
            prev_head: None,
        })
    }

    async fn run(&mut self) -> Result<()> {
        self.populate_queue().await;

        loop {
            // TODO: on disconnect rollback and wait for connections

            if self.sampling_fut.is_terminated() {
                self.schedule_next_sample_block().await?;
            }

            select! {
                _ = self.cancellation_token.cancelled() => break,
                res = &mut self.sampling_fut => match res {
                    Ok((height, accepted)) => {
                        let status = if accepted {
                            SamplingStatus::Accepted
                        } else {
                            SamplingStatus::Rejected
                        };

                        self.store
                            .update_sampling_metadata(height, status, Vec::new())
                            .await?;
                    }
                    Err(_) => {
                        // rollback store to the highest accepted sampling
                        // and clean related CIDs from blockstore
                        todo!();
                    },
                },
                _ = self.store.wait_new_head() => {
                    self.populate_queue().await;
                }
            }
        }

        Ok(())
    }

    async fn schedule_next_sample_block(&mut self) -> Result<()> {
        assert!(self.sampling_fut.is_terminated());

        let Some(args) = self.queue.pop_front() else {
            return Ok(());
        };

        // Make sure that block is still in the sampling window.
        if !in_sampling_window(args.time) {
            // Queue is sorted, so as soon as we reach a block
            // that is not in the sampling window, it means the
            // rest wouldn't be either.
            self.queue.clear();
            return Ok(());
        }

        let mut cids = Vec::new();

        // Select random shares to be sampled and generate their CID.
        for (row, col) in random_indexes(args.square_width, self.max_samples_needed) {
            let cid = sample_cid(row, col, args.height)?;
            cids.push(cid);
        }

        // Update CID list before we start the sampling procedure, otherwise there are
        // some edge cases that can leak the CIDs and never cleaned from blockstore.
        self.store
            .update_sampling_metadata(args.height, SamplingStatus::Unknown, cids.clone())
            .await?;

        let p2p = self.p2p.clone();

        // Schedule retrival of the CIDs. This will run later on in the `select!` loop.
        self.sampling_fut.set(async move {
            let now = Instant::now();
            let mut futs = FuturesUnordered::new();

            for cid in cids {
                let fut = p2p.get_shwap_cid(cid, Some(GET_SAMPLE_TIMEOUT));
                futs.push(fut);
            }

            let mut accepted = true;

            while let Some(res) = futs.next().await {
                match res {
                    Ok(_) => {}
                    // Validation is done at Bitswap level, through `ShwapMultihasher`.
                    // If the sample is not valid, it will never be delivered to us
                    // as the data of the CID. Because of that, the only signal
                    // that data sampling verification failed is query timing out.
                    Err(P2pError::BitswapQueryTimeout) => accepted = false,
                    Err(e) => return Err(e.into()),
                }
            }

            debug!(
                "Data sampling of {} is {}. Took {:?}",
                args.height,
                if accepted { "accepted" } else { "rejected" },
                now.elapsed()
            );

            Ok((args.height, accepted))
        });

        Ok(())
    }

    async fn populate_queue(&mut self) {
        // TODO: Adjust algorithm for backward syncing.

        let Ok(new_head) = self.store.head_height().await else {
            return;
        };

        match self.prev_head {
            Some(prev_head) => {
                for height in prev_head + 1..=new_head {
                    if is_sampled(&*self.store, height).await {
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
                    if is_sampled(&*self.store, height).await {
                        continue;
                    }

                    let Ok(header) = self.store.get_by_height(height).await else {
                        // We reached the tail of the known heights
                        break;
                    };

                    if !in_sampling_window(header.time()) {
                        // We reached the tail of the sampling window
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

/// Returns true is block is sampled (i.e. it was accepted or rejected).
async fn is_sampled(store: &impl Store, height: u64) -> bool {
    match store.get_sampling_metadata(height).await {
        Ok(Some(metadata)) => metadata.status != SamplingStatus::Unknown,
        _ => false,
    }
}

/// Returns unique and random indexes that will be used for the sampling.
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
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, MockP2pHandle};
    use celestia_tendermint_proto::Protobuf;
    use celestia_types::sample::{Sample, SampleId};
    use celestia_types::test_utils::{generate_eds, ExtendedHeaderGenerator};
    use celestia_types::{AxisType, DataAvailabilityHeader, ExtendedDataSquare};

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
        square_width: usize,
        simulate_invalid_sampling: bool,
    ) {
        let eds = generate_eds(square_width);
        let dah = DataAvailabilityHeader::from_eds(&eds);
        let header = gen.next_with_dah(dah);
        let height = header.height().value();

        store.append_single(header).await.unwrap();

        let mut cids = Vec::new();

        for i in 0..(square_width * square_width).min(MAX_SAMPLES_NEEDED) {
            let (cid, respond_to) = handle.expect_get_shwap_cid().await;

            // Simulate invalid sample by triggering BitswapQueryTimeout
            if simulate_invalid_sampling && i == 2 {
                respond_to.send(Err(P2pError::BitswapQueryTimeout)).unwrap();
                continue;
            }

            let sample_id: SampleId = cid.try_into().unwrap();
            assert_eq!(sample_id.block_height(), height);

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
