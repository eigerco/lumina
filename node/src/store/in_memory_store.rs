use std::ops::RangeInclusive;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::{Notify, RwLock};
use tracing::debug;

use crate::store::utils::{check_range_insert, verify_range_contiguous, RangeScanResult};
use crate::store::{HeaderRanges, Result, SamplingMetadata, SamplingStatus, Store, StoreError};

/// A non-persistent in memory [`Store`] implementation.
#[derive(Debug)]
pub struct InMemoryStore {
    /// Maps header Hash to the header itself, responsible for actually storing the header data
    headers: DashMap<Hash, ExtendedHeader>,
    /// Maps header height to the header sampling metadata, used by DAS
    sampling_data: DashMap<u64, SamplingMetadata>,
    /// Maps header height to its hash, in case we need to do lookup by height
    height_to_hash: DashMap<u64, Hash>,

    stored_ranges: RwLock<HeaderRanges>,

    /// Cached height of the lowest header that wasn't sampled yet
    lowest_unsampled_height: AtomicU64,
    /// Notify when a new header is added
    header_added_notifier: Notify,
}

// TODO: synchronisation

impl InMemoryStore {
    /// Create a new store.
    pub fn new() -> Self {
        InMemoryStore {
            headers: DashMap::new(),
            sampling_data: DashMap::new(),
            height_to_hash: DashMap::new(),
            stored_ranges: RwLock::new(HeaderRanges::default()),
            lowest_unsampled_height: AtomicU64::new(1),
            header_added_notifier: Notify::new(),
        }
    }

    #[inline]
    async fn get_head_height(&self) -> Result<u64> {
        Ok(*self
            .stored_ranges
            .read()
            .await
            .0
            .first()
            .ok_or(StoreError::NotFound)?
            .end())
    }

    pub(crate) async fn insert(
        &self,
        headers: Vec<ExtendedHeader>,
        verify_neighbours: bool,
    ) -> Result<()> {
        let (Some(head), Some(tail)) = (headers.first(), headers.last()) else {
            return Ok(());
        };

        let headers_range = head.height().value()..=tail.height().value();
        println!("I: {headers_range:?}");
        let neighbours_exist = self.try_insert_to_range(headers_range).await?;

        if verify_neighbours {
            self.verify_against_neighbours(head, tail, neighbours_exist)?;
        } else {
            verify_range_contiguous(&headers)?;
        }

        for header in headers {
            let hash = header.hash();
            let height = header.height().value();
            // this shouldn't deadlock as long as we don't hold references across awaits if any
            // https://github.com/xacrimon/dashmap/issues/233
            let hash_entry = self.headers.entry(hash);
            let height_entry = self.height_to_hash.entry(height);

            if matches!(hash_entry, Entry::Occupied(_)) {
                return Err(StoreError::HashExists(hash));
            }

            if matches!(height_entry, Entry::Occupied(_)) {
                panic!("shouldn't happen");
                // Reaching this point means another thread won the race and
                // there is a new head already.
                //return Err(StoreError::HeightExists(height));
            }

            debug!("Inserting header {hash} with height {height}");
            hash_entry.insert(header);
            height_entry.insert(hash);
        }

        self.header_added_notifier.notify_waiters();

        Ok(())
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        let head_height = self.get_head_height().await?;
        self.get_by_height(head_height)
    }

    fn contains_hash(&self, hash: &Hash) -> bool {
        self.headers.contains_key(hash)
    }

    fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.headers
            .get(hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    async fn contains_height(&self, height: u64) -> bool {
        self.stored_ranges
            .read()
            .await
            .0
            .iter()
            .any(|range| range.contains(&height))
    }

    fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let Some(hash) = self.height_to_hash.get(&height).as_deref().copied() else {
            return Err(StoreError::NotFound);
        };

        self.headers
            .get(&hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::LostHash(hash))
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
            Entry::Vacant(entry) => {
                entry.insert(SamplingMetadata { status, cids });
            }
            Entry::Occupied(mut entry) => {
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
        self.stored_ranges.read().await.clone()
    }

    async fn try_insert_to_range(&self, new_range: RangeInclusive<u64>) -> Result<(bool, bool)> {
        let mut stored_ranges_guard = self.stored_ranges.write().await;

        let stored_ranges = stored_ranges_guard.clone(); // XXX: ugh

        let RangeScanResult {
            range_index,
            range,
            range_to_remove,
            //neighbours_exist,
        } = check_range_insert(stored_ranges, new_range.clone())?; // XXX: cloneeeeee

        let prev_exists = new_range.start() != range.start();
        let next_exists = new_range.end() != range.end();

        if stored_ranges_guard.0.len() == range_index as usize {
            stored_ranges_guard.0.push(range);
        } else {
            stored_ranges_guard.0[range_index as usize] = range;
        }

        if let Some(to_remove) = range_to_remove {
            stored_ranges_guard.0.remove(to_remove as usize);
        }

        Ok((prev_exists, next_exists))
    }

    fn verify_against_neighbours(
        &self,
        lowest_header: &ExtendedHeader,
        highest_header: &ExtendedHeader,
        neighbours_exist: (bool, bool),
    ) -> Result<()> {
        debug_assert!(lowest_header.height().value() <= highest_header.height().value());
        let (prev_exists, next_exists) = neighbours_exist;

        if prev_exists {
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

        if next_exists {
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
        self.get_by_hash(hash)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height)
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
        self.contains_hash(hash)
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height).await
    }

    async fn insert(&self, header: Vec<ExtendedHeader>, verify_neighbours: bool) -> Result<()> {
        self.insert(header, verify_neighbours).await
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

impl From<Vec<ExtendedHeader>> for InMemoryStore {
    fn from(hs: Vec<ExtendedHeader>) -> Self {
        let range = match (hs.first(), hs.last()) {
            (Some(head), Some(tail)) => {
                HeaderRanges::from([head.height().value()..=tail.height().value()])
            }
            (None, None) => HeaderRanges::default(),
            _ => unreachable!(),
        };

        let height_to_hash = DashMap::default();
        for h in &hs {
            height_to_hash.insert(h.height().value(), h.hash());
        }

        let headers = DashMap::default();
        for h in hs {
            headers.insert(h.hash(), h);
        }

        InMemoryStore {
            headers,
            sampling_data: Default::default(),
            height_to_hash,
            stored_ranges: RwLock::new(range),
            lowest_unsampled_height: AtomicU64::new(0),
            header_added_notifier: Notify::new(),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for InMemoryStore {
    fn clone(&self) -> Self {
        InMemoryStore {
            headers: self.headers.clone(),
            sampling_data: self.sampling_data.clone(),
            height_to_hash: self.height_to_hash.clone(),
            stored_ranges: todo!(), // self.stored_ranges.clone(),
            lowest_unsampled_height: AtomicU64::new(
                self.lowest_unsampled_height.load(Ordering::Acquire),
            ),
            header_added_notifier: Notify::new(),
        }
    }
}
