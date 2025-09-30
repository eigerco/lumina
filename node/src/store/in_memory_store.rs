use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::Display;
use std::pin::pin;

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use libp2p::identity::Keypair;
use tokio::sync::{Notify, RwLock};
use tracing::debug;

use crate::block_ranges::BlockRanges;
use crate::store::utils::VerifiedExtendedHeaders;
use crate::store::{Result, SamplingMetadata, Store, StoreError, StoreInsertionError};

/// A non-persistent in memory [`Store`] implementation.
#[derive(Debug)]
pub struct InMemoryStore {
    /// Mutable part
    inner: RwLock<InMemoryStoreInner>,
    /// Notify when a new header is added
    header_added_notifier: Notify,
}

#[derive(Debug, Clone)]
struct InMemoryStoreInner {
    /// Maps header Hash to the header itself, responsible for actually storing the header data
    headers: HashMap<Hash, ExtendedHeader>,
    /// Maps header height to its hash, in case we need to do lookup by height
    height_to_hash: HashMap<u64, Hash>,
    /// Source of truth about headers present in the db, used to synchronise inserts
    header_ranges: BlockRanges,
    /// Maps header height to the header sampling metadata
    sampling_data: HashMap<u64, SamplingMetadata>,
    /// Source of truth about sampled ranges present in the db.
    sampled_ranges: BlockRanges,
    /// Source of truth about ranges that were pruned from db.
    pruned_ranges: BlockRanges,
    /// Node network identity
    libp2p_identity: Option<Keypair>,
}

impl InMemoryStoreInner {
    fn new() -> Self {
        Self {
            headers: HashMap::new(),
            height_to_hash: HashMap::new(),
            header_ranges: BlockRanges::default(),
            sampling_data: HashMap::new(),
            sampled_ranges: BlockRanges::default(),
            pruned_ranges: BlockRanges::default(),
            libp2p_identity: None,
        }
    }
}

impl InMemoryStore {
    /// Create a new store.
    pub fn new() -> Self {
        InMemoryStore {
            inner: RwLock::new(InMemoryStoreInner::new()),
            header_added_notifier: Notify::new(),
        }
    }

    #[inline]
    async fn get_head_height(&self) -> Result<u64> {
        self.inner.read().await.get_head_height()
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        let head_height = self.get_head_height().await?;
        self.get_by_height(head_height).await
    }

    async fn contains_hash(&self, hash: &Hash) -> bool {
        self.inner.read().await.contains_hash(hash)
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.inner.read().await.get_by_hash(hash)
    }

    async fn contains_height(&self, height: u64) -> bool {
        self.inner.read().await.contains_height(height)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.inner.read().await.get_by_height(height)
    }

    pub(crate) async fn insert<R>(&self, headers: R) -> Result<()>
    where
        R: TryInto<VerifiedExtendedHeaders> + Send,
        <R as TryInto<VerifiedExtendedHeaders>>::Error: Display,
    {
        let headers = headers
            .try_into()
            .map_err(|e| StoreInsertionError::HeadersVerificationFailed(e.to_string()))?;

        self.inner.write().await.insert(headers).await?;
        self.header_added_notifier.notify_waiters();

        Ok(())
    }

    async fn update_sampling_metadata(&self, height: u64, cids: Vec<Cid>) -> Result<()> {
        self.inner
            .write()
            .await
            .update_sampling_metadata(height, cids)
            .await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.inner.read().await.get_sampling_metadata(height).await
    }

    async fn mark_as_sampled(&self, height: u64) -> Result<()> {
        self.inner.write().await.mark_as_sampled(height).await
    }

    async fn get_stored_ranges(&self) -> BlockRanges {
        self.inner.read().await.header_ranges.clone()
    }

    async fn get_sampled_ranges(&self) -> BlockRanges {
        self.inner.read().await.sampled_ranges.clone()
    }

    async fn get_pruned_ranges(&self) -> BlockRanges {
        self.inner.read().await.pruned_ranges.clone()
    }

    /// Clone the store and all its contents, except for libp2p identity, which is re-generated.
    /// Async fn due to internal use of async mutex.
    pub async fn async_clone(&self) -> Self {
        InMemoryStore {
            inner: RwLock::new(self.inner.read().await.clone()),
            header_added_notifier: Notify::new(),
        }
    }

    async fn remove_height(&self, height: u64) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.remove_height(height)
    }

    async fn set_identity(&self, keypair: Keypair) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.set_identity(keypair).await
    }

    async fn get_identity(&self) -> Result<Keypair> {
        let mut inner = self.inner.write().await;
        inner.get_identity().await
    }
}

impl InMemoryStoreInner {
    #[inline]
    fn get_head_height(&self) -> Result<u64> {
        self.header_ranges.head().ok_or(StoreError::NotFound)
    }

    fn contains_hash(&self, hash: &Hash) -> bool {
        self.headers.contains_key(hash)
    }

    fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.headers.get(hash).cloned().ok_or(StoreError::NotFound)
    }

    fn contains_height(&self, height: u64) -> bool {
        self.header_ranges.contains(height)
    }

    fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let Some(hash) = self.height_to_hash.get(&height).copied() else {
            return Err(StoreError::NotFound);
        };

        Ok(self
            .headers
            .get(&hash)
            .expect("inconsistent between header hash and header heights")
            .to_owned())
    }

    async fn insert(&mut self, headers: VerifiedExtendedHeaders) -> Result<()> {
        let (Some(head), Some(tail)) = (headers.as_ref().first(), headers.as_ref().last()) else {
            return Ok(());
        };

        let headers_range = head.height().value()..=tail.height().value();
        let (prev_exists, next_exists) = self
            .header_ranges
            .check_insertion_constraints(&headers_range)
            .map_err(StoreInsertionError::ContraintsNotMet)?;

        // header range is already internally verified against itself in `P2p::get_unverified_header_ranges`
        self.verify_against_neighbours(prev_exists.then_some(head), next_exists.then_some(tail))?;

        for header in headers.into_iter() {
            let hash = header.hash();
            let height = header.height().value();

            debug_assert!(
                !self.height_to_hash.contains_key(&height),
                "inconsistency between headers table and ranges table"
            );

            let Entry::Vacant(headers_entry) = self.headers.entry(hash) else {
                // TODO: Remove this when we implement type-safe validation on insertion.
                return Err(StoreInsertionError::HashExists(hash).into());
            };

            debug!("Inserting header {hash} with height {height}");
            headers_entry.insert(header);
            self.height_to_hash.insert(height, hash);
        }

        self.header_ranges
            .insert_relaxed(&headers_range)
            .expect("invalid range");
        self.sampled_ranges
            .remove_relaxed(&headers_range)
            .expect("invalid range");
        self.pruned_ranges
            .remove_relaxed(&headers_range)
            .expect("invalid range");

        Ok(())
    }

    fn verify_against_neighbours(
        &self,
        lowest_header: Option<&ExtendedHeader>,
        highest_header: Option<&ExtendedHeader>,
    ) -> Result<()> {
        if let Some(lowest_header) = lowest_header {
            let prev = self
                .get_by_height(lowest_header.height().value() - 1)
                .map_err(|e| match e {
                    StoreError::NotFound => {
                        panic!("inconsistency between headers and ranges table")
                    }
                    e => e,
                })?;

            prev.verify(lowest_header)
                .map_err(|e| StoreInsertionError::NeighborsVerificationFailed(e.to_string()))?;
        }

        if let Some(highest_header) = highest_header {
            let next = self
                .get_by_height(highest_header.height().value() + 1)
                .map_err(|e| match e {
                    StoreError::NotFound => {
                        panic!("inconsistency between headers and ranges table")
                    }
                    e => e,
                })?;

            highest_header
                .verify(&next)
                .map_err(|e| StoreInsertionError::NeighborsVerificationFailed(e.to_string()))?;
        }

        Ok(())
    }

    async fn update_sampling_metadata(&mut self, height: u64, cids: Vec<Cid>) -> Result<()> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        match self.sampling_data.entry(height) {
            Entry::Vacant(entry) => {
                entry.insert(SamplingMetadata { cids });
            }
            Entry::Occupied(mut entry) => {
                let metadata = entry.get_mut();

                for cid in cids {
                    if !metadata.cids.contains(&cid) {
                        metadata.cids.push(cid);
                    }
                }
            }
        }

        Ok(())
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let Some(metadata) = self.sampling_data.get(&height) else {
            return Ok(None);
        };

        Ok(Some(metadata.clone()))
    }

    async fn mark_as_sampled(&mut self, height: u64) -> Result<()> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        self.sampled_ranges
            .insert_relaxed(height..=height)
            .expect("invalid height");

        Ok(())
    }

    fn remove_height(&mut self, height: u64) -> Result<()> {
        if !self.header_ranges.contains(height) {
            return Err(StoreError::NotFound);
        }

        let Entry::Occupied(height_to_hash) = self.height_to_hash.entry(height) else {
            return Err(StoreError::StoredDataError(format!(
                "inconsistency between ranges and height_to_hash tables, height {height}"
            )));
        };

        let hash = height_to_hash.get();
        let Entry::Occupied(header) = self.headers.entry(*hash) else {
            return Err(StoreError::StoredDataError(format!(
                "inconsistency between header and height_to_hash tables, hash {hash}"
            )));
        };

        // sampling data may or may not be there
        self.sampling_data.remove(&height);

        height_to_hash.remove_entry();
        header.remove_entry();

        self.header_ranges
            .remove_relaxed(height..=height)
            .expect("invalid height");
        self.sampled_ranges
            .remove_relaxed(height..=height)
            .expect("invalid height");
        self.pruned_ranges
            .insert_relaxed(height..=height)
            .expect("invalid height");

        Ok(())
    }

    async fn get_identity(&mut self) -> Result<Keypair> {
        Ok(self
            .libp2p_identity
            .get_or_insert_with(Keypair::generate_ed25519)
            .clone())
    }

    async fn set_identity(&mut self, keypair: Keypair) -> Result<()> {
        let _ = self.libp2p_identity.insert(keypair);
        Ok(())
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head().await
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.get_by_hash(hash).await
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height).await
    }

    async fn wait_new_head(&self) -> u64 {
        let head = self.get_head_height().await.unwrap_or(0);
        let mut notifier = pin!(self.header_added_notifier.notified());

        loop {
            let new_head = self.get_head_height().await.unwrap_or(0);

            if head != new_head {
                return new_head;
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.header_added_notifier.notified());
        }
    }

    async fn wait_height(&self, height: u64) -> Result<()> {
        let mut notifier = pin!(self.header_added_notifier.notified());

        loop {
            if self.contains_height(height).await {
                return Ok(());
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.header_added_notifier.notified());
        }
    }

    async fn head_height(&self) -> Result<u64> {
        self.get_head_height().await
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.contains_hash(hash).await
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height).await
    }

    async fn insert<R>(&self, header: R) -> Result<()>
    where
        R: TryInto<VerifiedExtendedHeaders> + Send,
        <R as TryInto<VerifiedExtendedHeaders>>::Error: Display,
    {
        self.insert(header).await
    }

    async fn update_sampling_metadata(&self, height: u64, cids: Vec<Cid>) -> Result<()> {
        self.update_sampling_metadata(height, cids).await
    }

    async fn mark_as_sampled(&self, height: u64) -> Result<()> {
        self.mark_as_sampled(height).await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height).await
    }

    async fn get_stored_header_ranges(&self) -> Result<BlockRanges> {
        Ok(self.get_stored_ranges().await)
    }

    async fn get_sampled_ranges(&self) -> Result<BlockRanges> {
        Ok(self.get_sampled_ranges().await)
    }

    async fn get_pruned_ranges(&self) -> Result<BlockRanges> {
        Ok(self.get_pruned_ranges().await)
    }

    async fn remove_height(&self, height: u64) -> Result<()> {
        self.remove_height(height).await
    }

    async fn close(self) -> Result<()> {
        Ok(())
    }

    async fn set_identity(&self, keypair: Keypair) -> Result<()> {
        self.set_identity(keypair).await
    }

    async fn get_identity(&self) -> Result<Keypair> {
        self.get_identity().await
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}
