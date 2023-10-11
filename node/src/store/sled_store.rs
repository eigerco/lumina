use std::convert::Infallible;
use std::io;
use std::ops::Deref;
use std::path::Path;

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use directories::ProjectDirs;
use sled::transaction::{ConflictableTransactionError, TransactionError};
use sled::{Db, Error as SledError, Transactional, Tree};
use tempdir::TempDir;
use tendermint_proto::Protobuf;
use tracing::debug;

use crate::store::Store;
use crate::store::{Result, StoreError};

const HEAD_HEIGHT_KEY: &[u8] = b"KEY.HEAD_HEIGHT";
const HASH_TREE_ID: &[u8] = b"HASH";
const HEIGHT_TO_HASH_TREE_ID: &[u8] = b"HEIGHT";

#[derive(Debug)]
pub struct SledStore {
    db: Db,
    headers: Tree,
    height_to_hash: Tree,
}

impl SledStore {
    pub fn new(network_id: &str) -> sled::Result<Self> {
        let Some(project_dirs) = ProjectDirs::from("co", "eiger", "celestia") else {
            return Err(sled::Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "Failed to get cache directory",
            )));
        };
        let mut db_path = project_dirs.cache_dir().to_owned();
        db_path.push(network_id);

        Self::new_in_path(db_path)
    }

    pub fn new_temp() -> sled::Result<Self> {
        let tmp_path = TempDir::new("celestia")?.into_path();

        let db = sled::Config::default()
            .path(tmp_path)
            .temporary(true)
            .create_new(true) // make sure we fail if db is already there
            .open()?;
        Self::init(db)
    }

    pub fn new_in_path<P>(path: P) -> sled::Result<Self>
    where
        P: AsRef<Path>,
    {
        let db = sled::open(path)?;
        Self::init(db)
    }

    fn init(db: Db) -> sled::Result<Self> {
        let headers = db.open_tree(HASH_TREE_ID)?;
        let height_to_hash = db.open_tree(HEIGHT_TO_HASH_TREE_ID)?;

        Ok(Self {
            db,
            headers,
            height_to_hash,
        })
    }

    pub fn head_height(&self) -> Result<u64> {
        read_height_by_db_key(&self.db, HEAD_HEIGHT_KEY)
    }

    pub fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        read_header_by_db_key(&self.headers, hash.as_bytes())
    }

    pub fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let hash = read_hash_by_db_key(&self.height_to_hash, &height_to_key(height))?;

        self.get_by_hash(&hash)
    }

    pub fn get_head(&self) -> Result<ExtendedHeader> {
        let head_height = self.head_height()?;
        self.get_by_height(head_height)
    }

    pub fn contains_hash(&self, hash: &Hash) -> bool {
        self.headers.contains_key(hash.as_bytes()).unwrap_or(false)
    }

    pub fn contains_height(&self, height: u64) -> bool {
        self.height_to_hash
            .contains_key(height_to_key(height))
            .unwrap_or(false)
    }

    pub fn append_single_unchecked(&self, header: &ExtendedHeader) -> Result<()> {
        let hash = header.hash();
        let height = header.height().value();

        let head_height = self.head_height().unwrap_or(0);

        // A light check before checking the whole map
        if head_height > 0 && height <= head_height {
            return Err(StoreError::HeightExists(height));
        }

        // Check if it's continuous before checking the whole map.
        if head_height + 1 != height {
            return Err(StoreError::NonContinuousAppend(head_height, height));
        }

        // Do actual inserts as a transaction, failing if keys already exist
        (self.db.deref(), &self.headers, &self.height_to_hash).transaction(
            move |(db, headers, height_to_hash)| {
                let height_key = height_to_key(height);
                if height_to_hash
                    .insert(&height_key, hash.as_bytes())?
                    .is_some()
                {
                    return Err(ConflictableTransactionError::Abort(
                        StoreError::HeightExists(height),
                    ));
                }

                db.insert(HEAD_HEIGHT_KEY, &height_key)?;

                // make sure Result is Infallible, we unwrap it later
                let serialized_header: std::result::Result<_, Infallible> = header.encode_vec();

                if headers
                    .insert(hash.as_bytes(), serialized_header.unwrap())?
                    .is_some()
                {
                    return Err(ConflictableTransactionError::Abort(StoreError::HashExists(
                        hash,
                    )));
                }

                Ok(())
            },
        )?;

        debug!("Inserting header {hash} with height {height}");
        Ok(())
    }

    pub async fn flush_to_storage(&self) -> Result<()> {
        self.db.flush_async().await?;

        Ok(())
    }
}

// we can report contained StoreError directly, otherwise transpose Sled error as StoreError
impl From<TransactionError<StoreError>> for StoreError {
    fn from(error: TransactionError<StoreError>) -> StoreError {
        type E = TransactionError<StoreError>;
        match error {
            E::Abort(store_error) => store_error,
            E::Storage(sled_error) => sled_error.into(),
        }
    }
}

// divide errors into recoverable and not avoiding directly relying on passing sled types
impl From<SledError> for StoreError {
    fn from(error: SledError) -> StoreError {
        match error {
            e @ SledError::CollectionNotFound(_) | e @ SledError::Corruption { .. } => {
                StoreError::StoredDataError(e.to_string())
            }
            e @ SledError::Unsupported(_)
            | e @ SledError::ReportableBug(_)
            | e @ SledError::Io(_) => StoreError::BackingStoreError(e.to_string()),
        }
    }
}

#[async_trait]
impl Store for SledStore {
    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head()
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.get_by_hash(hash)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height)
    }

    async fn head_height(&self) -> Result<u64> {
        self.head_height()
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.contains_hash(hash)
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height)
    }

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        self.append_single_unchecked(&header)
    }
}

#[inline]
fn read_height_by_db_key(tree: &Tree, db_key: &[u8]) -> Result<u64> {
    match tree
        .get(db_key)?
        .ok_or(StoreError::NotFound)?
        .as_ref()
        .try_into()
    {
        Ok(b) => Ok(u64::from_be_bytes(b)),
        Err(_) => Err(StoreError::NotFound),
    }
}

#[inline]
fn read_hash_by_db_key(tree: &Tree, db_key: &[u8]) -> Result<Hash> {
    match tree
        .get(db_key)?
        .ok_or(StoreError::NotFound)?
        .as_ref()
        .try_into()
    {
        Ok(b) => Ok(Hash::Sha256(b)),
        Err(_) => Err(StoreError::NotFound),
    }
}

#[inline]
fn read_header_by_db_key(tree: &Tree, db_key: &[u8]) -> Result<ExtendedHeader> {
    let serialized = tree.get(db_key)?.ok_or(StoreError::NotFound)?;

    ExtendedHeader::decode(serialized.as_ref()).map_err(|e| StoreError::CelestiaTypes(e.into()))
}

#[inline]
fn height_to_key(height: u64) -> [u8; 8] {
    // sled recommends BigEndian representation for ints since it preserves expected int order
    // when sorted lexicographically
    height.to_be_bytes()
}

// clone existing store to a new temporary store
#[cfg(feature = "test-utils")]
impl Clone for SledStore {
    fn clone(&self) -> Self {
        let clone = SledStore::new_temp().unwrap();
        clone.db.import(self.db.export());
        clone
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;

    #[test]
    fn test_empty_store() {
        let s = SledStore::new_temp().unwrap();
        assert!(matches!(s.head_height(), Err(StoreError::NotFound)));
        assert!(matches!(s.get_head(), Err(StoreError::NotFound)));
        assert!(matches!(s.get_by_height(1), Err(StoreError::NotFound)));
        assert!(matches!(
            s.get_by_hash(&Hash::Sha256([0; 32])),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn test_read_write() {
        let s = SledStore::new_temp().unwrap();
        let mut gen = ExtendedHeaderGenerator::new();

        let header = gen.next();

        s.append_single_unchecked(&header.clone()).unwrap();
        assert_eq!(s.head_height().unwrap(), 1);
        assert_eq!(s.get_head().unwrap(), header);
        assert_eq!(s.get_by_height(1).unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_pregenerated_data() {
        let (s, _) = gen_filled_store(100, None);
        assert_eq!(s.head_height().unwrap(), 100);
        let head = s.get_head().unwrap();
        assert_eq!(s.get_by_height(100).unwrap(), head);
        assert!(matches!(s.get_by_height(101), Err(StoreError::NotFound)));

        let header = s.get_by_height(54).unwrap();
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_duplicate_insert() {
        let (s, mut gen) = gen_filled_store(100, None);
        let header101 = gen.next();
        s.append_single_unchecked(&header101.clone()).unwrap();
        assert!(matches!(
            s.append_single_unchecked(&header101.clone()),
            Err(StoreError::HeightExists(101))
        ));
    }

    #[test]
    fn test_overwrite_height() {
        let (s, gen) = gen_filled_store(100, None);

        // Height 30 with different hash
        let header29 = s.get_by_height(29).unwrap();
        let header30 = gen.next_of(&header29);

        let insert_existing_result = s.append_single_unchecked(&header30);
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HeightExists(30))
        ));
    }

    #[test]
    fn test_overwrite_hash() {
        let (s, _) = gen_filled_store(100, None);
        let mut dup_header = s.get_by_height(33).unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_single_unchecked(&dup_header.clone());
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HashExists(_))
        ));
    }

    #[tokio::test]
    async fn test_append_range() {
        let (s, mut gen) = gen_filled_store(10, None);
        let hs = gen.next_many(4);
        s.append_unchecked(hs).await.unwrap();
        s.get_by_height(14).unwrap();
    }

    #[tokio::test]
    async fn test_append_gap_between_head() {
        let (s, mut gen) = gen_filled_store(10, None);

        // height 11
        gen.next();
        // height 12
        let upcoming_head = gen.next();

        let insert_with_gap_result = s.append_single_unchecked(&upcoming_head);
        assert!(matches!(
            insert_with_gap_result,
            Err(StoreError::NonContinuousAppend(10, 12))
        ));
    }

    #[tokio::test]
    async fn test_non_continuous_append() {
        let (s, mut gen) = gen_filled_store(10, None);
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

        let s = SledStore::new_temp().unwrap();

        assert!(matches!(
            s.append_single_unchecked(&header5),
            Err(StoreError::NonContinuousAppend(0, 5))
        ));
    }

    #[test]
    fn test_store_persistence() {
        let db_dir = TempDir::new("celestia.test").unwrap();
        let (original_store, mut gen) = gen_filled_store(0, Some(db_dir.path()));
        let mut original_headers = gen.next_many(20);

        for h in &original_headers {
            original_store
                .append_single_unchecked(h)
                .expect("inserting test data failed");
        }
        drop(original_store);

        let reopened_store = SledStore::new_in_path(db_dir.path()).expect("failed to reopen store");

        assert_eq!(
            original_headers.last().unwrap().height().value(),
            reopened_store.head_height().unwrap()
        );
        for original_header in &original_headers {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }

        let mut new_headers = gen.next_many(10);
        for h in &new_headers {
            reopened_store
                .append_single_unchecked(h)
                .expect("failed to insert data");
        }
        drop(reopened_store);

        original_headers.append(&mut new_headers);

        let reopened_store = SledStore::new_in_path(db_dir.path()).expect("failed to reopen store");
        assert_eq!(
            original_headers.last().unwrap().height().value(),
            reopened_store.head_height().unwrap()
        );
        for original_header in &original_headers {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }
    }

    #[test]
    fn test_separate_stores() {
        let (store0, mut gen0) = gen_filled_store(10, None);
        let store1 = store0.clone();
        let mut gen1 = gen0.fork();

        for h in gen0.next_many(5) {
            store0.append_single_unchecked(&h).unwrap()
        }
        for h in gen1.next_many(6) {
            store1.append_single_unchecked(&h).unwrap();
        }

        assert_eq!(store0.get_by_height(10).unwrap(), store1.get_by_height(10).unwrap());
        assert_ne!(store0.get_by_height(11).unwrap(), store1.get_by_height(11).unwrap());

        assert_eq!(store0.head_height().unwrap(), 15);
        assert_eq!(store1.head_height().unwrap(), 16);
    }

    pub fn gen_filled_store(
        amount: u64,
        path: Option<&Path>,
    ) -> (SledStore, ExtendedHeaderGenerator) {
        let s = if let Some(path) = path {
            SledStore::new_in_path(path).unwrap()
        } else {
            SledStore::new_temp().unwrap()
        };

        let mut gen = ExtendedHeaderGenerator::new();

        let headers = gen.next_many(amount);

        for header in headers {
            s.append_single_unchecked(&header)
                .expect("inserting test data failed");
        }

        (s, gen)
    }
}
