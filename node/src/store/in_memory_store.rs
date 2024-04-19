use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::Notify;
use tracing::debug;

use crate::store::{Result, SamplingMetadata, Store, StoreError};

/// A non-persistent in memory [`Store`] implementation.
#[derive(Debug)]
pub struct InMemoryStore {
    /// Maps header Hash to the header itself, responsible for actually storing the header data
    headers: DashMap<Hash, ExtendedHeader>,
    /// Maps header height to the header sampling metadata, used by DAS
    sampling_data: DashMap<u64, SamplingMetadata>,
    /// Maps header height to its hash, in case we need to do lookup by height
    height_to_hash: DashMap<u64, Hash>,
    /// Cached height of the highest header in store
    head_height: AtomicU64,
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
            header_added_notifier: Notify::new(),
        }
    }

    #[inline]
    fn get_head_height(&self) -> Result<u64> {
        let height = self.head_height.load(Ordering::Acquire);

        if height == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(height)
        }
    }

    pub(crate) fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        let hash = header.hash();
        let height = header.height().value();
        let head_height = self.get_head_height().unwrap_or(0);

        // A light check before checking the whole map
        if head_height > 0 && height <= head_height {
            return Err(StoreError::HeightExists(height));
        }

        // Check if it's continuous before checking the whole map.
        if head_height + 1 != height {
            return Err(StoreError::NonContinuousAppend(head_height, height));
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

        self.head_height.store(height, Ordering::Release);
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

    fn update_sampling_metadata(&self, height: u64, accepted: bool, cids: Vec<Cid>) -> Result<()> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        match self.sampling_data.entry(height) {
            Entry::Vacant(entry) => {
                entry.insert(SamplingMetadata {
                    accepted,
                    cids_sampled: cids,
                });
            }
            Entry::Occupied(mut entry) => {
                let metadata = entry.get_mut();
                metadata.accepted = accepted;

                for cid in &cids {
                    if !metadata.cids_sampled.contains(cid) {
                        metadata.cids_sampled.push(cid.to_owned());
                    }
                }
            }
        }

        Ok(())
    }

    fn contains_sampling_metadata(&self, height: u64) -> bool {
        self.sampling_data.contains_key(&height)
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

    async fn wait_new_head(&self) -> u64 {
        let head = self.get_head_height().unwrap_or(0);
        let mut notifier = pin!(self.header_added_notifier.notified());

        loop {
            let new_head = self.get_head_height().unwrap_or(0);

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

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        self.append_single_unchecked(header)
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<()> {
        self.update_sampling_metadata(height, accepted, cids)
    }

    async fn has_sampling_metadata(&self, height: u64) -> bool {
        self.contains_sampling_metadata(height)
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height)
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
            header_added_notifier: Notify::new(),
        }
    }
}
