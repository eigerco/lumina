use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use celestia_types::ExtendedHeader;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tendermint::Hash;
use thiserror::Error;
use tracing::{info, instrument};

type Result<T, E = StoreError> = std::result::Result<T, E>;

#[async_trait]
pub trait Store: Send + Sync + Debug {
    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader>;
    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader>;

    async fn height(&self) -> u64;
    async fn has(&self, hash: &Hash) -> bool;
    async fn has_at(&self, height: u64) -> bool;

    async fn append_single(&self, header: ExtendedHeader) -> Result<()>;

    async fn append<I: IntoIterator<Item = ExtendedHeader>>(&self, headers: I) -> Result<()>
    where
        I: IntoIterator<Item = ExtendedHeader> + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        let headers = headers.into_iter();

        for (idx, header) in headers.enumerate() {
            if self.append_single(header).await.is_err() {
                return Err(StoreError::ContinuousAppendFailedAt(idx));
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct InMemoryStore {
    headers: DashMap<Hash, ExtendedHeader>,
    height_to_hash: DashMap<u64, Hash>,
    head_height: AtomicU64,
}

#[derive(Error, Debug, PartialEq)]
pub enum StoreError {
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),

    #[error("Height {0} already exists in store")]
    HeightExists(u64),

    #[error("Continuous append impossible")]
    NonContinuousAppend,

    #[error("Failed to apply header {0}")]
    ContinuousAppendFailedAt(usize),

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
        }
    }

    pub fn with_genesis(genesis: ExtendedHeader) -> Self {
        let genesis_hash = genesis.hash();
        let genesis_height = genesis.height().value();

        InMemoryStore {
            headers: DashMap::from_iter([(genesis_hash, genesis)]),
            height_to_hash: DashMap::from_iter([(genesis_height, genesis_hash)]),
            head_height: AtomicU64::new(genesis_height),
        }
    }

    pub fn get_head_height(&self) -> u64 {
        self.head_height.load(Ordering::Acquire)
    }

    #[instrument(err)]
    pub fn append_continuous(&self, header: ExtendedHeader) -> Result<(), StoreError> {
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
        if self.get_head_height() + 1 != height.value() {
            return Err(StoreError::NonContinuousAppend);
        }

        info!("Will insert {hash} at {height}");
        hash_entry.insert(header);
        height_entry.insert(hash);

        self.head_height.store(height.value(), Ordering::Release);

        Ok(())
    }

    #[instrument(err)]
    pub fn get_head(&self) -> Result<ExtendedHeader, StoreError> {
        let head_height = self.head_height.load(Ordering::Acquire);
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

    pub fn exists_by_hash(&self, hash: &Hash) -> bool {
        self.headers.get(hash).is_some()
    }

    #[instrument(err)]
    pub fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader, StoreError> {
        self.headers
            .get(hash)
            .as_deref()
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    pub fn exists_by_height(&self, height: u64) -> bool {
        let Some(hash) = self.height_to_hash.get(&height).as_deref().copied() else {
            return false;
        };

        self.headers.get(&hash).is_some()
    }

    #[instrument(err)]
    pub fn get_by_height(&self, height: u64) -> Result<ExtendedHeader, StoreError> {
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
    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader, StoreError> {
        self.get_by_hash(hash)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader, StoreError> {
        self.get_by_height(height)
    }

    async fn height(&self) -> u64 {
        self.get_head_height()
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.exists_by_hash(hash)
    }

    async fn has_at(&self, height: u64) -> bool {
        self.exists_by_height(height)
    }

    async fn append_single(&self, header: ExtendedHeader) -> Result<()> {
        self.append_continuous(header)
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
        s.append_continuous(header.clone()).unwrap();
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
        assert_eq!(s.append_continuous(header.clone()), Ok(()));
        assert_eq!(
            s.append_continuous(header.clone()),
            Err(StoreError::HashExists(header.hash()))
        );
    }

    #[test]
    fn test_overwrite_height() {
        let s = gen_filled_store(100);
        let insert_existing_result = s.append_continuous(gen_extended_header(30));
        assert_eq!(insert_existing_result, Err(StoreError::HeightExists(30)));
    }

    #[test]
    fn test_overwrite_hash() {
        let s = gen_filled_store(100);
        let mut dup_header = s.get_by_height(33).unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_continuous(dup_header.clone());
        assert_eq!(
            insert_existing_result,
            Err(StoreError::HashExists(dup_header.hash()))
        );
    }
}
