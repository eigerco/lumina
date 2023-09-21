use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::ExtendedHeader;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tendermint::Hash;
use thiserror::Error;
use tracing::{debug, instrument};

type Result<T, E = StoreError> = std::result::Result<T, E>;

#[async_trait]
pub trait Store: Send + Sync + Debug {
    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader>;
    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader>;
    async fn get_head(&self) -> Result<ExtendedHeader>;

    async fn height(&self) -> u64;
    async fn has(&self, hash: &Hash) -> bool;
    async fn has_at(&self, height: u64) -> bool;

    // TODO: should caller or the store verify and validate?
    /// append single header maintaining continuity from the genesis to the head of the store
    /// caller is responsible for validation and verification against current head
    async fn append_single_unverified(&self, header: ExtendedHeader) -> Result<()>;

    /// append a range of headers maintaining continuity from the genesis to the head of the store
    /// caller is responsible for validation and verification
    async fn append_unverified<I>(&self, headers: I) -> Result<()>
    where
        I: IntoIterator<Item = ExtendedHeader> + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        for header in headers.into_iter() {
            self.append_single_unverified(header).await?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct InMemoryStore {
    headers: DashMap<Hash, ExtendedHeader>,
    height_to_hash: DashMap<u64, Hash>,
    head_height: AtomicU64,
    genesis_height: AtomicU64,
}

#[derive(Error, Debug, PartialEq)]
pub enum StoreError {
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),

    #[error("Height {0} already exists in store")]
    HeightExists(u64),

    #[error("Failed to append header at height {1}, current head {0}")]
    NonContinuousAppend(u64, u64),

    #[error("Failed to validate header at height {0}")]
    HeaderChecksError(u64),

    #[error("Header not found in store")]
    NotFound,

    #[error("Store in inconsistent state, lost head")]
    LostStoreHead,

    #[error("Store in inconsistent state; height->hash mapping exists, {0} missing")]
    LostHash(Hash),
}

impl InMemoryStore {
    pub fn new() -> Self {
        InMemoryStore {
            headers: DashMap::new(),
            height_to_hash: DashMap::new(),
            head_height: AtomicU64::new(0),
            genesis_height: AtomicU64::new(0),
        }
    }

    pub fn get_head_height(&self) -> u64 {
        self.head_height.load(Ordering::Acquire)
    }

    pub fn get_genesis_height(&self) -> u64 {
        self.genesis_height.load(Ordering::Acquire)
    }

    #[instrument(err)]
    pub fn append_single_unverified(&self, header: ExtendedHeader) -> Result<(), StoreError> {
        let hash = header.hash();
        let height = header.height();

        // lock both maps to ensure consistency
        // this shouldn't deadlock as long as we don't hold references across awaits if any
        // https://github.com/xacrimon/dashmap/issues/233
        let hash_entry = self.headers.entry(hash);
        let height_entry = self.height_to_hash.entry(height.into());

        if matches!(hash_entry, Entry::Occupied(_)) {
            return Err(StoreError::HashExists(hash));
        }
        if matches!(height_entry, Entry::Occupied(_)) {
            return Err(StoreError::HeightExists(height.into()));
        }
        if self
            .genesis_height
            .compare_exchange(0, height.value(), Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            // new genesis was set, head height will be updated below
        } else if self.get_head_height() + 1 != height.value() {
            return Err(StoreError::NonContinuousAppend(
                self.get_head_height(),
                height.value(),
            ));
        }

        debug!("Inserting header {hash} with height {height}");
        hash_entry.insert(header);
        height_entry.insert(hash);

        self.head_height.store(height.value(), Ordering::Release);

        Ok(())
    }

    #[instrument(err)]
    pub fn get_head(&self) -> Result<ExtendedHeader, StoreError> {
        let head_height = self.get_head_height();
        if head_height == 0 {
            return Err(StoreError::NotFound);
        }

        let Some(head_hash) = self.height_to_hash.get(&head_height).as_deref().copied() else {
            return Err(StoreError::LostStoreHead);
        };

        self.headers
            .get(&head_hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::LostHash(head_hash))
    }

    pub fn contains_hash(&self, hash: &Hash) -> bool {
        self.headers.contains_key(hash)
    }

    #[instrument(err)]
    pub fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader, StoreError> {
        self.headers
            .get(hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    pub fn contains_height(&self, height: u64) -> bool {
        height != 0
            && height >= self.genesis_height.load(Ordering::Acquire)
            && height <= self.get_head_height()
    }

    #[instrument(err)]
    pub fn get_by_height(&self, height: u64) -> Result<ExtendedHeader, StoreError> {
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let Some(hash) = self.height_to_hash.get(&height).as_deref().copied() else {
            return Err(StoreError::NotFound);
        };

        self.headers
            .get(&hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::LostHash(hash))
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.get_by_hash(hash)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height)
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head()
    }

    async fn height(&self) -> u64 {
        self.get_head_height()
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.contains_hash(hash)
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height)
    }

    async fn append_single_unverified(&self, header: ExtendedHeader) -> Result<()> {
        self.append_single_unverified(header)
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::test_utils::{gen_extended_header, gen_filled_store};
    use tendermint::block::Height;

    #[test]
    fn test_empty_store() {
        let s = InMemoryStore::new();
        assert_eq!(s.get_head_height(), 0);
        assert_eq!(s.get_head(), Err(StoreError::NotFound));
        assert_eq!(s.get_by_height(1), Err(StoreError::NotFound));
        assert_eq!(
            s.get_by_hash(&Hash::Sha256([0; 32])),
            Err(StoreError::NotFound)
        );
    }

    #[test]
    fn test_read_write() {
        let s = InMemoryStore::new();
        let header = gen_extended_header(1);
        s.append_single_unverified(header.clone()).unwrap();
        assert_eq!(s.get_head_height(), 1);
        assert_eq!(s.get_head().unwrap(), header);
        assert_eq!(s.get_by_height(1).unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_pregenerated_data() {
        let s = gen_filled_store(100);
        assert_eq!(s.get_head_height(), 100);
        let head = s.get_head().unwrap();
        assert_eq!(s.get_by_height(100), Ok(head));
        assert_eq!(s.get_by_height(101), Err(StoreError::NotFound));

        let header = s.get_by_height(54).unwrap();
        assert_eq!(s.get_by_hash(&header.hash()), Ok(header));
    }

    #[test]
    fn test_duplicate_insert() {
        let s = gen_filled_store(100);
        let header = gen_extended_header(101);
        assert_eq!(s.append_single_unverified(header.clone()), Ok(()));
        assert_eq!(
            s.append_single_unverified(header.clone()),
            Err(StoreError::HashExists(header.hash()))
        );
    }

    #[test]
    fn test_overwrite_height() {
        let s = gen_filled_store(100);
        let insert_existing_result = s.append_single_unverified(gen_extended_header(30));
        assert_eq!(insert_existing_result, Err(StoreError::HeightExists(30)));
    }

    #[test]
    fn test_overwrite_hash() {
        let s = gen_filled_store(100);
        let mut dup_header = s.get_by_height(33).unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_single_unverified(dup_header.clone());
        assert_eq!(
            insert_existing_result,
            Err(StoreError::HashExists(dup_header.hash()))
        );
    }

    #[tokio::test]
    async fn test_append_range() {
        let s = gen_filled_store(10);
        let hs = [
            gen_extended_header(11),
            gen_extended_header(12),
            gen_extended_header(13),
            gen_extended_header(14),
        ];
        let insert_range_result = s.append_unverified(hs).await;
        assert_eq!(insert_range_result, Ok(()));
    }

    #[tokio::test]
    async fn test_append_gap_between_head() {
        let s = gen_filled_store(10);
        let upcoming_head = gen_extended_header(12);
        let insert_with_gap_result = s.append_single_unverified(upcoming_head);
        assert_eq!(
            insert_with_gap_result,
            Err(StoreError::NonContinuousAppend(10, 12))
        );
    }

    #[tokio::test]
    async fn test_non_continuous_append() {
        let s = gen_filled_store(10);
        let hs = [
            gen_extended_header(11),
            gen_extended_header(12),
            gen_extended_header(13),
            gen_extended_header(15),
            gen_extended_header(16),
        ];

        let insert_existing_result = s.append_unverified(hs).await;
        assert_eq!(
            insert_existing_result,
            Err(StoreError::NonContinuousAppend(13, 15))
        );
    }

    #[test]
    fn test_genesis_with_height() {
        let gen = gen_extended_header(10);
        let s = InMemoryStore::new();
        s.append_single_unverified(gen)
            .expect("failed to set genesis with custom height");

        let append_before_genesis_result = s.append_single_unverified(gen_extended_header(9));
        assert_eq!(
            append_before_genesis_result,
            Err(StoreError::NonContinuousAppend(10, 9))
        );

        let append_after_genesis_result = s.append_single_unverified(gen_extended_header(11));
        assert_eq!(append_after_genesis_result, Ok(()));
    }
}
