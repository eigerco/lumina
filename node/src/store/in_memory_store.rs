use std::collections::hash_map::Entry as HashMapEntry;
use std::collections::HashMap;
use std::pin::pin;

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use dashmap::mapref::entry::Entry as DashMapEntry;
use dashmap::DashMap;
use tokio::sync::{Notify, RwLock};
use tracing::debug;

use crate::store::header_ranges::{HeaderRanges, HeaderRangesExt, VerifiedExtendedHeaders};
use crate::store::{Result, SamplingMetadata, SamplingStatus, Store, StoreError};

/// A non-persistent in memory [`Store`] implementation.
#[derive(Debug)]
pub struct InMemoryStore {
    /// Mutable part
    inner: RwLock<InMemoryStoreInner>,
    /// Maps header height to the header sampling metadata, used by DAS
    sampling_data: DashMap<u64, SamplingMetadata>,
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
    stored_ranges: HeaderRanges,
}

impl InMemoryStoreInner {
    fn new() -> Self {
        Self {
            headers: HashMap::new(),
            height_to_hash: HashMap::new(),
            stored_ranges: HeaderRanges::default(),
        }
    }
}

impl InMemoryStore {
    /// Create a new store.
    pub fn new() -> Self {
        InMemoryStore {
            inner: RwLock::new(InMemoryStoreInner::new()),
            sampling_data: DashMap::new(),
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
        StoreError: From<<R as TryInto<VerifiedExtendedHeaders>>::Error>,
    {
        let headers = headers.try_into()?;
        self.inner.write().await.insert(headers).await?;
        self.header_added_notifier.notify_waiters();
        Ok(())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        if !self.contains_height(height).await {
            return Err(StoreError::NotFound);
        }

        match self.sampling_data.entry(height) {
            DashMapEntry::Vacant(entry) => {
                entry.insert(SamplingMetadata { status, cids });
            }
            DashMapEntry::Occupied(mut entry) => {
                let metadata = entry.get_mut();
                metadata.status = status;

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
        if !self.contains_height(height).await {
            return Err(StoreError::NotFound);
        }

        let Some(metadata) = self.sampling_data.get(&height) else {
            return Ok(None);
        };

        Ok(Some(metadata.clone()))
    }

    async fn get_stored_ranges(&self) -> HeaderRanges {
        self.inner.read().await.get_stored_ranges()
    }

    /// Clone the store and all its contents. Async fn due to internal use of async mutex.
    pub async fn async_clone(&self) -> Self {
        InMemoryStore {
            inner: RwLock::new(self.inner.read().await.clone()),
            sampling_data: self.sampling_data.clone(),
            header_added_notifier: Notify::new(),
        }
    }
}

impl InMemoryStoreInner {
    fn get_stored_ranges(&self) -> HeaderRanges {
        self.stored_ranges.clone()
    }

    #[inline]
    fn get_head_height(&self) -> Result<u64> {
        self.stored_ranges.head().ok_or(StoreError::NotFound)
    }

    fn contains_hash(&self, hash: &Hash) -> bool {
        self.headers.contains_key(hash)
    }

    fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.headers.get(hash).cloned().ok_or(StoreError::NotFound)
    }

    fn contains_height(&self, height: u64) -> bool {
        self.stored_ranges.contains(height)
    }

    fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let Some(hash) = self.height_to_hash.get(&height).copied() else {
            return Err(StoreError::NotFound);
        };

        self.headers
            .get(&hash)
            .cloned()
            .ok_or(StoreError::LostHash(hash))
    }

    async fn insert(&mut self, headers: VerifiedExtendedHeaders) -> Result<()> {
        let (Some(head), Some(tail)) = (headers.as_ref().first(), headers.as_ref().last()) else {
            return Ok(());
        };

        let headers_range = head.height().value()..=tail.height().value();
        let range_scan_result = self.stored_ranges.check_range_insert(&headers_range)?;

        let prev_exists = headers_range.start() != range_scan_result.range.start();
        let next_exists = headers_range.end() != range_scan_result.range.end();
        // header range is already internally verified against itself in `P2p::get_unverified_header_ranges`
        self.verify_against_neighbours(prev_exists.then_some(head), next_exists.then_some(tail))?;

        // make sure we don't already have any of the provided hashes before doing any inserts to
        // avoid having to do a rollback
        for header in headers.as_ref() {
            let hash = header.hash();
            if self.headers.contains_key(&hash) {
                return Err(StoreError::HashExists(hash));
            }
        }

        for header in headers.into_iter() {
            let hash = header.hash();
            let height = header.height().value();

            let HashMapEntry::Vacant(hash_entry) = self.headers.entry(hash) else {
                panic!(
                    "hash present in store right after we checked its absence, should not happen"
                );
            };
            let HashMapEntry::Vacant(height_entry) = self.height_to_hash.entry(height) else {
                return Err(StoreError::StoredDataError(
                    "inconsistency between headers and ranges table".into(),
                ));
            };

            debug!("Inserting header {hash} with height {height}");
            hash_entry.insert(header);
            height_entry.insert(hash);
        }

        self.stored_ranges.update_range(range_scan_result);

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
                .map_err(|e| {
                    if let StoreError::NotFound = e {
                        StoreError::StoredDataError(
                            "inconsistency between headers and ranges table".into(),
                        )
                    } else {
                        e
                    }
                })?;
            prev.verify(lowest_header)?;
        }

        if let Some(highest_header) = highest_header {
            let next = self
                .get_by_height(highest_header.height().value() + 1)
                .map_err(|e| {
                    if let StoreError::NotFound = e {
                        StoreError::StoredDataError(
                            "inconsistency between headers and ranges table".into(),
                        )
                    } else {
                        e
                    }
                })?;
            highest_header.verify(&next)?;
        }
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
        StoreError: From<<R as TryInto<VerifiedExtendedHeaders>>::Error>,
    {
        self.insert(header).await
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        self.update_sampling_metadata(height, status, cids).await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height).await
    }

    async fn get_stored_header_ranges(&self) -> Result<HeaderRanges> {
        Ok(self.get_stored_ranges().await)
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}
