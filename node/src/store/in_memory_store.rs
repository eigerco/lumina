use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::Notify;
use tracing::{debug, info};

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
    /// Cached height of the lowest header that wasn't sampled yet
    lowest_unsampled_height: AtomicU64,
    /// Notifier for height
    check_height_notifier: Notify,
}

impl InMemoryStore {
    /// Create a new store.
    pub fn new() -> Self {
        InMemoryStore {
            headers: DashMap::new(),
            sampling_data: DashMap::new(),
            height_to_hash: DashMap::new(),
            head_height: AtomicU64::new(0),
            lowest_unsampled_height: AtomicU64::new(1),
            check_height_notifier: Notify::new(),
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

    #[inline]
    fn get_next_unsampled_height(&self) -> u64 {
        self.lowest_unsampled_height.load(Ordering::Acquire)
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
        self.check_height_notifier.notify_waiters();

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
                metadata.cids_sampled.extend_from_slice(&cids);
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
        let mut notifier = pin!(self.check_height_notifier.notified());

        loop {
            if self.contains_height(height) {
                return Ok(());
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.check_height_notifier.notified());
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
            lowest_unsampled_height: AtomicU64::new(
                self.lowest_unsampled_height.load(Ordering::Acquire),
            ),
            check_height_notifier: Notify::new(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[async_test]
    async fn test_contains_height() {
        let s = gen_filled_store(2).0;

        assert!(!s.has_at(0).await);
        assert!(s.has_at(1).await);
        assert!(s.has_at(2).await);
        assert!(!s.has_at(3).await);
    }

    #[test]
    fn test_empty_store() {
        let s = InMemoryStore::new();
        assert!(matches!(s.get_head_height(), Err(StoreError::NotFound)));
        assert!(matches!(s.get_head(), Err(StoreError::NotFound)));
        assert!(matches!(s.get_by_height(1), Err(StoreError::NotFound)));
        assert!(matches!(
            s.get_by_hash(&Hash::Sha256([0; 32])),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn test_read_write() {
        let s = InMemoryStore::new();
        let mut gen = ExtendedHeaderGenerator::new();

        let header = gen.next();

        s.append_single_unchecked(header.clone()).unwrap();
        assert_eq!(s.get_head_height().unwrap(), 1);
        assert_eq!(s.get_head().unwrap(), header);
        assert_eq!(s.get_by_height(1).unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_pregenerated_data() {
        let (s, _) = gen_filled_store(100);
        assert_eq!(s.get_head_height().unwrap(), 100);
        let head = s.get_head().unwrap();
        assert_eq!(s.get_by_height(100).unwrap(), head);
        assert!(matches!(s.get_by_height(101), Err(StoreError::NotFound)));

        let header = s.get_by_height(54).unwrap();
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_duplicate_insert() {
        let (s, mut gen) = gen_filled_store(100);
        let header101 = gen.next();
        s.append_single_unchecked(header101.clone()).unwrap();
        assert!(matches!(
            s.append_single_unchecked(header101.clone()),
            Err(StoreError::HeightExists(101))
        ));
    }

    #[test]
    fn test_overwrite_height() {
        let (s, gen) = gen_filled_store(100);

        // Height 30 with different hash
        let header29 = s.get_by_height(29).unwrap();
        let header30 = gen.next_of(&header29);

        let insert_existing_result = s.append_single_unchecked(header30);
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HeightExists(30))
        ));
    }

    #[test]
    fn test_overwrite_hash() {
        let (s, _) = gen_filled_store(100);
        let mut dup_header = s.get_by_height(33).unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_single_unchecked(dup_header.clone());
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HashExists(_))
        ));
    }

    #[async_test]
    async fn test_append_range() {
        let (s, mut gen) = gen_filled_store(10);
        let hs = gen.next_many(4);
        s.append_unchecked(hs).await.unwrap();
        s.get_by_height(14).unwrap();
    }

    #[async_test]
    async fn test_append_gap_between_head() {
        let (s, mut gen) = gen_filled_store(10);

        // height 11
        gen.next();
        // height 12
        let upcoming_head = gen.next();

        let insert_with_gap_result = s.append_single_unchecked(upcoming_head);
        assert!(matches!(
            insert_with_gap_result,
            Err(StoreError::NonContinuousAppend(10, 12))
        ));
    }

    #[async_test]
    async fn test_non_continuous_append() {
        let (s, mut gen) = gen_filled_store(10);
        let mut hs = gen.next_many(6);

        // remove height 14
        hs.remove(3);

        let insert_existing_result = s.append_unchecked(hs).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::NonContinuousAppend(13, 15))
        ));
    }

    #[test]
    fn test_genesis_with_height() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        let s = InMemoryStore::new();

        assert!(matches!(
            s.append_single_unchecked(header5),
            Err(StoreError::NonContinuousAppend(0, 5))
        ));
    }

    #[async_test]
    async fn test_sampling_height_empty_store() {
        let (store, _) = gen_filled_store(0);
        store.update_sampling_metadata(0, true, vec![]).unwrap_err();
        store.update_sampling_metadata(1, true, vec![]).unwrap_err();
    }

    #[async_test]
    async fn test_sampling_height() {
        let (store, _) = gen_filled_store(9);

        store.update_sampling_metadata(0, true, vec![]).unwrap_err();
        store.update_sampling_metadata(1, true, vec![]).unwrap();
        store.update_sampling_metadata(2, true, vec![]).unwrap();
        store.update_sampling_metadata(3, false, vec![]).unwrap();
        store.update_sampling_metadata(4, true, vec![]).unwrap();
        store.update_sampling_metadata(5, false, vec![]).unwrap();
        store.update_sampling_metadata(6, false, vec![]).unwrap();

        store.update_sampling_metadata(8, true, vec![]).unwrap();

        assert_eq!(store.get_next_unsampled_height(), 7);
    }

    #[test]
    fn test_sampling_merge() {
        let (store, _) = gen_filled_store(1);
        let cid0 = "zdpuAyvkgEDQm9TenwGkd5eNaosSxjgEYd8QatfPetgB1CdEZ"
            .parse()
            .unwrap();
        let cid1 = "zb2rhe5P4gXftAwvA4eXQ5HJwsER2owDyS9sKaQRRVQPn93bA"
            .parse()
            .unwrap();

        store
            .update_sampling_metadata(1, false, vec![cid0])
            .unwrap();
        store.update_sampling_metadata(1, false, vec![]).unwrap();

        let sampling_data = store.get_sampling_metadata(1).unwrap().unwrap();
        assert!(!sampling_data.accepted);
        assert_eq!(sampling_data.cids_sampled, vec![cid0]);

        store.update_sampling_metadata(1, true, vec![cid1]).unwrap();

        assert_eq!(store.get_next_unsampled_height(), 2);

        let sampling_data = store.get_sampling_metadata(1).unwrap().unwrap();
        assert!(sampling_data.accepted);
        assert_eq!(sampling_data.cids_sampled, vec![cid0, cid1]);
    }

    #[test]
    fn test_sampled_cids() {
        let (store, _) = gen_filled_store(5);

        let cids: Vec<Cid> = [
            "bafkreieq5jui4j25lacwomsqgjeswwl3y5zcdrresptwgmfylxo2depppq",
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
            "zdpuAyvkgEDQm9TenwGkd5eNaosSxjgEYd8QatfPetgB1CdEZ",
            "zb2rhe5P4gXftAwvA4eXQ5HJwsER2owDyS9sKaQRRVQPn93bA",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();

        store
            .update_sampling_metadata(1, true, cids.clone())
            .unwrap();
        store
            .update_sampling_metadata(2, true, cids[0..1].to_vec())
            .unwrap();
        store
            .update_sampling_metadata(4, false, cids[3..].to_vec())
            .unwrap();
        store.update_sampling_metadata(5, false, vec![]).unwrap();

        assert_eq!(store.get_next_unsampled_height(), 3);

        let sampling_data = store.get_sampling_metadata(1).unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids);
        assert!(sampling_data.accepted);

        let sampling_data = store.get_sampling_metadata(2).unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids[0..1]);
        assert!(sampling_data.accepted);

        assert!(store.get_sampling_metadata(3).unwrap().is_none());

        let sampling_data = store.get_sampling_metadata(4).unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids[3..]);
        assert!(!sampling_data.accepted);

        let sampling_data = store.get_sampling_metadata(5).unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, vec![]);
        assert!(!sampling_data.accepted);
    }

    pub fn gen_filled_store(amount: u64) -> (InMemoryStore, ExtendedHeaderGenerator) {
        let s = InMemoryStore::new();
        let mut gen = ExtendedHeaderGenerator::new();

        let headers = gen.next_many(amount);

        for header in headers {
            s.append_single_unchecked(header)
                .expect("inserting test data failed");
        }

        (s, gen)
    }
}
