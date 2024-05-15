use std::pin::pin;
use std::sync::atomic::{fence, AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use smallvec::smallvec;
use tokio::sync::Notify;
use tracing::{debug, info};

use crate::store::{
    HeaderRange, HeaderRanges, InsertMode, Result, SamplingMetadata, Store, StoreError,
};

/// A non-persistent in memory [`Store`] implementation.
#[derive(Debug)]
pub struct InMemoryStore {
    /// Maps header Hash to the header itself, responsible for actually storing the header data
    headers: DashMap<Hash, ExtendedHeader>,
    /// Maps header height to the header sampling metadata, used by DAS
    sampling_data: DashMap<u64, SamplingMetadata>,
    /// Maps header height to its hash, in case we need to do lookup by height
    height_to_hash: DashMap<u64, Hash>,
    /// Cached height of the highest header in the store
    head_height: AtomicU64,
    /// Cached height of the lowest header in the store
    tail_height: AtomicU64,
    /// Cached height of the lowest header that wasn't sampled yet
    lowest_unsampled_height: AtomicU64,
    /// Notify when a new header is added
    header_added_notifier: Notify,
}

impl InMemoryStore {
    /// Create a new store.
    pub fn new() -> Self {
        InMemoryStore {
            headers: DashMap::new(),
            sampling_data: DashMap::new(),
            height_to_hash: DashMap::new(),
            head_height: AtomicU64::new(0),
            tail_height: AtomicU64::new(0),
            lowest_unsampled_height: AtomicU64::new(1),
            header_added_notifier: Notify::new(),
        }
    }

    #[inline]
    fn get_head_height(&self) -> Result<u64> {
        let height = *self.get_stored_range().end();

        if height == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(height)
        }
    }

    /*
    #[inline]
    fn get_tail_height(&self) -> Result<u64> {
        let height = *self.get_stored_range().start();

        if height == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(height)
        }
    }
    */

    #[inline]
    fn get_next_unsampled_height(&self) -> u64 {
        self.lowest_unsampled_height.load(Ordering::Acquire)
    }

    pub(crate) fn insert_single(&self, header: ExtendedHeader, mode: InsertMode) -> Result<()> {
        let hash = header.hash();
        let height = header.height().value();
        let stored_range = self.get_stored_range();

        // A light check before checking the whole map
        if stored_range.contains(&height) {
            return Err(StoreError::HeightExists(height));
        }

        // Only allow appending and prepending to existing range
        let append = stored_range.end() + 1 == height;
        if !append && stored_range.start() - 1 != height {
            return Err(StoreError::NonContinuousAppend(0, height));
        }

        match mode {
            InsertMode::NextTrusted => match self.get_by_height(height - 1) {
                Ok(prev) => prev.verify(&header)?,
                Err(StoreError::NotFound) => (),
                Err(e) => return Err(e),
            },
            InsertMode::PreviousTrusted => match self.get_by_height(height + 1) {
                Ok(next) => header.verify(&next)?,
                Err(StoreError::NotFound) => (),
                Err(e) => return Err(e),
            },
            InsertMode::BothTrusted | InsertMode::InsertHead => (),
        }

        // lock both maps to ensure consistency
        // this shouldn't deadlock as long as we don't hold references across awaits if any
        // https://github.com/xacrimon/dashmap/issues/233
        let hash_entry = self.headers.entry(hash);
        let height_entry = self.height_to_hash.entry(height);

        if matches!(hash_entry, Entry::Occupied(_)) {
            return Err(StoreError::HashExists(hash));
        }

        if matches!(height_entry, Entry::Occupied(_)) {
            // Reaching this point means another thread won the race and
            // there is a new head already.
            return Err(StoreError::HeightExists(height));
        }

        debug!("Inserting header {hash} with height {height}");
        hash_entry.insert(header);
        height_entry.insert(hash);

        if append {
            self.head_height.store(height, Ordering::Relaxed);
        } else {
            self.tail_height.store(height, Ordering::Relaxed)
        }
        fence(Ordering::Release);
        self.header_added_notifier.notify_waiters();

        Ok(())
    }

    fn get_head(&self) -> Result<ExtendedHeader> {
        let head_height = self.get_head_height()?;
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

    fn contains_height(&self, height: u64) -> bool {
        let Ok(head_height) = self.get_head_height() else {
            return false;
        };

        height != 0 && height <= head_height
    }

    fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let Some(hash) = self.height_to_hash.get(&height).as_deref().copied() else {
            return Err(StoreError::LostHeight(height));
        };

        self.headers
            .get(&hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::LostHash(hash))
    }

    fn update_sampling_metadata(&self, height: u64, accepted: bool, cids: Vec<Cid>) -> Result<u64> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let new_inserted = match self.sampling_data.entry(height) {
            Entry::Vacant(entry) => {
                entry.insert(SamplingMetadata {
                    accepted,
                    cids_sampled: cids,
                });
                true
            }
            Entry::Occupied(mut entry) => {
                let metadata = entry.get_mut();
                metadata.accepted = accepted;

                for cid in &cids {
                    if !metadata.cids_sampled.contains(cid) {
                        metadata.cids_sampled.push(cid.to_owned());
                    }
                }

                false
            }
        };

        if new_inserted {
            self.update_lowest_unsampled_height()
        } else {
            info!("Overriding existing sampling metadata for height {height}");
            // modified header wasn't new, no need to update the height
            Ok(self.get_next_unsampled_height())
        }
    }

    fn update_lowest_unsampled_height(&self) -> Result<u64> {
        loop {
            let previous_height = self.lowest_unsampled_height.load(Ordering::Acquire);
            let mut current_height = previous_height;
            while self.sampling_data.contains_key(&current_height) {
                current_height += 1;
            }

            if self.lowest_unsampled_height.compare_exchange(
                previous_height,
                current_height,
                Ordering::Release,
                Ordering::Relaxed,
            ) == Ok(previous_height)
            {
                break Ok(current_height);
            }
        }
    }

    fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let Some(metadata) = self.sampling_data.get(&height) else {
            return Ok(None);
        };

        Ok(Some(metadata.clone()))
    }

    /*
    fn prepend(&self, headers: Vec<ExtendedHeader>) -> Result<()> {
        // assert headers are verified internally
        for header in headers.into_iter().rev() {
            println!("insert {:?}", header.height());
            self.prepend_single_unchecked(header)?;
        }

        Ok(())
    }

    fn prepend_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        let hash = header.hash();
        let height = header.height().value();
        let tail_height = self.get_tail_height().unwrap_or(0);

        if !self.is_empty() {
            if height > tail_height {
                return Err(StoreError::HeightExists(height));
            }

            println!("{height}, {tail_height}");
            if height != tail_height - 1 {
                return Err(StoreError::NonContinuousAppend(tail_height, height));
            }
        }

        let hash_entry = self.headers.entry(hash);
        let height_entry = self.height_to_hash.entry(height);

        if matches!(hash_entry, Entry::Occupied(_)) {
            return Err(StoreError::HashExists(hash));
        }

        if matches!(height_entry, Entry::Occupied(_)) {
            // Reaching this point means another thread won the race and
            // there is a new tail already.
            return Err(StoreError::HeightExists(height));
        }

        println!("Inserting header {hash} with height {height}");
        hash_entry.insert(header);
        height_entry.insert(hash);

        self.tail_height.store(height, Ordering::Release);
        if self
            .head_height
            .compare_exchange(0, height, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            info!("Inserting first header at {height}");
        }

        self.header_added_notifier.notify_waiters();
        println!(
            "head: {:?}, tail: {:?}",
            self.get_head_height(),
            self.get_tail_height()
        );

        Ok(())
    }
    */

    fn get_stored_range(&self) -> HeaderRange {
        let head_height = self.head_height.load(Ordering::Relaxed);
        let tail_height = self.tail_height.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        head_height..=tail_height
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head()
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.get_by_hash(hash)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height)
    }

    async fn wait_height(&self, height: u64) -> Result<()> {
        let mut notifier = pin!(self.header_added_notifier.notified());

        loop {
            if self.contains_height(height) {
                return Ok(());
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.header_added_notifier.notified());
        }
    }

    async fn head_height(&self) -> Result<u64> {
        self.get_head_height()
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.contains_hash(hash)
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height)
    }

    async fn insert_single(&self, header: ExtendedHeader, mode: InsertMode) -> Result<()> {
        self.insert_single(header, mode)
    }

    async fn next_unsampled_height(&self) -> Result<u64> {
        Ok(self.get_next_unsampled_height())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        self.update_sampling_metadata(height, accepted, cids)
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height)
    }

    async fn get_stored_header_ranges(&self) -> Result<HeaderRanges> {
        Ok(HeaderRanges(smallvec![self.get_stored_range()]))
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
            head_height: AtomicU64::new(self.head_height.load(Ordering::Acquire)),
            tail_height: AtomicU64::new(self.tail_height.load(Ordering::Acquire)),
            lowest_unsampled_height: AtomicU64::new(
                self.lowest_unsampled_height.load(Ordering::Acquire),
            ),
            header_added_notifier: Notify::new(),
        }
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;

    #[test]
    fn prepend() {
        let store = InMemoryStore::new();
        let mut gen = ExtendedHeaderGenerator::new();
        let _ = gen.next_many(10);
        let hs = gen.next_many(40);

        store.prepend(hs).unwrap();
    }
}
*/
