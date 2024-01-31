use std::convert::Infallible;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use directories::ProjectDirs;
use sled::transaction::{ConflictableTransactionError, TransactionError};
use sled::{Db, Error as SledError, Transactional, Tree};
use tempdir::TempDir;
use tokio::task::spawn_blocking;
use tokio::task::JoinError;
use tracing::{debug, info};
use cid::Cid;

use crate::store::{ExtendedHeaderMetadata, Result, Store, StoreError};

const HEAD_HEIGHT_KEY: &[u8] = b"KEY.HEAD_HEIGHT";
const SAMPLED_HEIGHT_KEY: &[u8] = b"KEY.SAMPLED_HEIGHT";
const HASH_TREE_ID: &[u8] = b"HASH";
const HEIGHT_TO_HASH_TREE_ID: &[u8] = b"HEIGHT";
const HEIGHT_TO_METADATA_TREE_ID: &[u8] = b"METADATA";

/// A [`Store`] implementation based on a [`sled`] database.
#[derive(Debug)]
pub struct SledStore {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Reference to the entire sled db
    db: Db,
    /// sub-tree which maps header hash to header
    headers: Tree,
    /// sub-tree which maps header height to its hash
    height_to_hash: Tree,
    /// sub-tree which maps header height to its metadata
    height_to_metadata: Tree,
}

impl SledStore {
    /// Create or open a persistent store.
    pub async fn new(network_id: String) -> Result<Self> {
        spawn_blocking(move || {
            let Some(project_dirs) = ProjectDirs::from("co", "eiger", "celestia") else {
                return Err(StoreError::OpenFailed(
                    "Unable to get system cache path to open header store".to_string(),
                ));
            };
            let mut db_path = project_dirs.cache_dir().to_owned();
            db_path.push(network_id);

            sled::open(db_path)
                .and_then(Self::init)
                .map_err(|e| StoreError::OpenFailed(e.to_string()))
        })
        .await?
    }

    /// Create a persistent store in a temporary directory.
    pub async fn new_temp() -> Result<Self> {
        spawn_blocking(move || {
            let tmp_path = TempDir::new("celestia")?.into_path();

            sled::Config::default()
                .path(tmp_path)
                .temporary(true)
                .create_new(true) // make sure we fail if db is already there
                .open()
                .and_then(Self::init)
        })
        .await?
        .map_err(|e| StoreError::OpenFailed(e.to_string()))
    }

    /// Create a persistent store in a given path.
    pub async fn new_in_path<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().to_owned();
        spawn_blocking(move || {
            let db = sled::open(path)?;
            Self::init(db)
        })
        .await?
        .map_err(|e| StoreError::OpenFailed(e.to_string()))
    }

    // `open_tree` might be blocking, make sure to call this from `spawn_blocking` or similar
    fn init(db: Db) -> sled::Result<Self> {
        let headers = db.open_tree(HASH_TREE_ID)?;
        let height_to_hash = db.open_tree(HEIGHT_TO_HASH_TREE_ID)?;
        let height_to_metadata = db.open_tree(HEIGHT_TO_METADATA_TREE_ID)?;

        Ok(Self {
            inner: Arc::new(Inner {
                db,
                headers,
                height_to_hash,
                height_to_metadata,
            }),
        })
    }

    async fn head_height(&self) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY)).await?
    }

    async fn get_heighest_sampled_height(&self) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || read_height_by_db_key(&inner.db, SAMPLED_HEIGHT_KEY)).await?
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();
        let hash = *hash;

        spawn_blocking(move || read_header_by_db_key(&inner.headers, hash.as_bytes())).await?
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let hash = read_hash_by_db_key(&inner.height_to_hash, &height_to_key(height))?;
            read_header_by_db_key(&inner.headers, hash.as_bytes())
        })
        .await?
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let head_height = read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY)?;
            let hash = read_hash_by_db_key(&inner.height_to_hash, &height_to_key(head_height))?;
            read_header_by_db_key(&inner.headers, hash.as_bytes())
        })
        .await?
    }

    async fn contains_hash(&self, hash: &Hash) -> bool {
        let inner = self.inner.clone();
        let hash = *hash;

        spawn_blocking(move || inner.headers.contains_key(hash.as_bytes()).unwrap_or(false))
            .await
            .unwrap_or(false)
    }

    async fn contains_height(&self, height: u64) -> bool {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            inner
                .height_to_hash
                .contains_key(height_to_key(height))
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        let hash = header.hash();
        let height = header.height().value();
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let head_height = read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY).unwrap_or(0);

            // A light check before checking the whole map
            if head_height > 0 && height <= head_height {
                return Err(StoreError::HeightExists(height));
            }

            // Check if it's continuous before checking the whole map.
            if head_height + 1 != height {
                return Err(StoreError::NonContinuousAppend(head_height, height));
            }

            // Do actual inserts as a transaction, failing if keys already exist
            (inner.db.deref(), &inner.headers, &inner.height_to_hash).transaction(
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
            Ok(())
        })
        .await??;

        debug!("Inserting header {hash} with height {height}");
        Ok(())
    }

    async fn mark_header_sampled(&self, height: u64, accepted: bool, cids: Vec<Cid>) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let head_height = read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY).unwrap_or(0);

            // A light check before checking the whole map
            if head_height > 0 && height <= head_height {
                return Err(StoreError::HeightExists(height));
            }

            let metadata_key = height_to_key(height);

            let metadata = ExtendedHeaderMetadata {
                accepted,
                cids_sampled: cids,
            };
            let serialized = serde_json::to_vec(&metadata).unwrap();

            if let Some(previous) = inner
                .height_to_metadata
                .insert(metadata_key, serialized)? {
                    info!("Overriding existing sampling metadata for height {height}");
            };


                /*
                .fetch_and_update(metadata_key, |old_metadata| {
                    let mut m = match old_metadata {
                        Some(serialized) => {
                            info!("Overriding existing sampling metadata for height {height}");
                            serde_json::from_slice(serialized).unwrap()
                        },
                        None => ExtendedHeaderMetadata::default(),
                    };
                    m.accepted = accepted;
                    Some(serde_json::to_vec(&m).unwrap())
                })?;
                */
            Ok(())
        })
        .await??;

        self.increment_sampled_height()
    }

    fn increment_sampled_height(&self) -> Result<u64> {
        todo!()
    }

    /// Flush the store's state to the filesystem.
    pub async fn flush_to_storage(&self) -> Result<()> {
        self.inner.db.flush_async().await?;

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
            e @ SledError::Unsupported(_) | e @ SledError::ReportableBug(_) => {
                StoreError::BackingStoreError(e.to_string())
            }
            SledError::Io(e) => e.into(),
        }
    }
}

impl From<JoinError> for StoreError {
    fn from(error: JoinError) -> StoreError {
        StoreError::ExecutorError(error.to_string())
    }
}

#[async_trait]
impl Store for SledStore {
    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head().await
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        self.get_by_hash(hash).await
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.get_by_height(height).await
    }

    async fn head_height(&self) -> Result<u64> {
        self.head_height().await
    }

    async fn has(&self, hash: &Hash) -> bool {
        self.contains_hash(hash).await
    }

    async fn has_at(&self, height: u64) -> bool {
        self.contains_height(height).await
    }

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        self.append_single_unchecked(header).await
    }

    async fn highest_sampled_height(&self) -> Result<u64> {
        self.get_heighest_sampled_height().await
    }

    async fn mark_header_sampled(&self, height: u64, accepted: bool, cids: Vec<Cid>) -> Result<u64> {
        self.mark_header_sampled(height, accepted, cids).await
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

/*
#[inline]
fn read_metadata_by_db_key(tree: &TransactionalTree, db_key: &[u8]) -> Result<ExtendedHeaderMetadata> {
    let serialized = tree.get(db_key)?.ok_or(StoreError::NotFound)?;
    // TODO: no json
    serde_json::from_str(serialized.as_ref())

    //ExtendedHeaderMetadata::decode(serialized.as_ref()).map_err(|e| StoreError::CelestiaTypes(e.into()))
}
*/

#[inline]
fn height_to_key(height: u64) -> [u8; 8] {
    // sled recommends BigEndian representation for ints since it preserves expected int order
    // when sorted lexicographically
    height.to_be_bytes()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;

    #[tokio::test]
    async fn test_empty_store() {
        let s = SledStore::new_temp().await.unwrap();
        assert!(matches!(s.head_height().await, Err(StoreError::NotFound)));
        assert!(matches!(s.get_head().await, Err(StoreError::NotFound)));
        assert!(matches!(
            s.get_by_height(1).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            s.get_by_hash(&Hash::Sha256([0; 32])).await,
            Err(StoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn test_read_write() {
        let s = SledStore::new_temp().await.unwrap();
        let mut gen = ExtendedHeaderGenerator::new();

        let header = gen.next();

        s.append_single_unchecked(header.clone()).await.unwrap();
        assert_eq!(s.head_height().await.unwrap(), 1);
        assert_eq!(s.get_head().await.unwrap(), header);
        assert_eq!(s.get_by_height(1).await.unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[tokio::test]
    async fn test_pregenerated_data() {
        let (s, _) = gen_filled_store(100, None).await;
        assert_eq!(s.head_height().await.unwrap(), 100);
        let head = s.get_head().await.unwrap();
        assert_eq!(s.get_by_height(100).await.unwrap(), head);
        assert!(matches!(
            s.get_by_height(101).await,
            Err(StoreError::NotFound)
        ));

        let header = s.get_by_height(54).await.unwrap();
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[tokio::test]
    async fn test_duplicate_insert() {
        let (s, mut gen) = gen_filled_store(100, None).await;
        let header101 = gen.next();
        s.append_single_unchecked(header101.clone()).await.unwrap();
        assert!(matches!(
            s.append_single_unchecked(header101).await,
            Err(StoreError::HeightExists(101))
        ));
    }

    #[tokio::test]
    async fn test_overwrite_height() {
        let (s, gen) = gen_filled_store(100, None).await;

        // Height 30 with different hash
        let header29 = s.get_by_height(29).await.unwrap();
        let header30 = gen.next_of(&header29);

        let insert_existing_result = s.append_single_unchecked(header30).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HeightExists(30))
        ));
    }

    #[tokio::test]
    async fn test_overwrite_hash() {
        let (s, _) = gen_filled_store(100, None).await;
        let mut dup_header = s.get_by_height(33).await.unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_single_unchecked(dup_header).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HashExists(_))
        ));
    }

    #[tokio::test]
    async fn test_append_range() {
        let (s, mut gen) = gen_filled_store(10, None).await;
        let hs = gen.next_many(4);
        s.append_unchecked(hs).await.unwrap();
        s.get_by_height(14).await.unwrap();
    }

    #[tokio::test]
    async fn test_append_gap_between_head() {
        let (s, mut gen) = gen_filled_store(10, None).await;

        // height 11
        gen.next();
        // height 12
        let upcoming_head = gen.next();

        let insert_with_gap_result = s.append_single_unchecked(upcoming_head).await;
        assert!(matches!(
            insert_with_gap_result,
            Err(StoreError::NonContinuousAppend(10, 12))
        ));
    }

    #[tokio::test]
    async fn test_non_continuous_append() {
        let (s, mut gen) = gen_filled_store(10, None).await;
        let mut hs = gen.next_many(6);

        // remove height 14
        hs.remove(3);

        let insert_existing_result = s.append_unchecked(hs).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::NonContinuousAppend(13, 15))
        ));
    }

    #[tokio::test]
    async fn test_genesis_with_height() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        let s = SledStore::new_temp().await.unwrap();

        assert!(matches!(
            s.append_single_unchecked(header5).await,
            Err(StoreError::NonContinuousAppend(0, 5))
        ));
    }

    #[tokio::test]
    async fn test_store_persistence() {
        let db_dir = TempDir::new("celestia.test").unwrap();
        let (original_store, mut gen) = gen_filled_store(0, Some(db_dir.path())).await;
        let mut original_headers = gen.next_many(20);

        for h in &original_headers {
            original_store
                .append_single_unchecked(h.clone())
                .await
                .expect("inserting test data failed");
        }
        drop(original_store);

        let reopened_store = SledStore::new_in_path(db_dir.path())
            .await
            .expect("failed to reopen store");

        assert_eq!(
            original_headers.last().unwrap().height().value(),
            reopened_store.head_height().await.unwrap()
        );
        for original_header in &original_headers {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .await
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }

        let mut new_headers = gen.next_many(10);
        for h in &new_headers {
            reopened_store
                .append_single_unchecked(h.clone())
                .await
                .expect("failed to insert data");
        }
        drop(reopened_store);

        original_headers.append(&mut new_headers);

        let reopened_store = SledStore::new_in_path(db_dir.path().to_owned())
            .await
            .expect("failed to reopen store");
        assert_eq!(
            original_headers.last().unwrap().height().value(),
            reopened_store.head_height().await.unwrap()
        );
        for original_header in &original_headers {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .await
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }
    }

    #[tokio::test]
    async fn test_separate_stores() {
        let (store0, mut gen0) = gen_filled_store(0, None).await;
        let store1 = SledStore::new_temp().await.unwrap();

        let headers = gen0.next_many(10);
        store0.append(headers.clone()).await.unwrap();
        store1.append(headers).await.unwrap();

        let mut gen1 = gen0.fork();

        for h in gen0.next_many(5) {
            store0.append_single_unchecked(h.clone()).await.unwrap()
        }
        for h in gen1.next_many(6) {
            store1.append_single_unchecked(h.clone()).await.unwrap();
        }

        assert_eq!(
            store0.get_by_height(10).await.unwrap(),
            store1.get_by_height(10).await.unwrap()
        );
        assert_ne!(
            store0.get_by_height(11).await.unwrap(),
            store1.get_by_height(11).await.unwrap()
        );

        assert_eq!(store0.head_height().await.unwrap(), 15);
        assert_eq!(store1.head_height().await.unwrap(), 16);
    }

    pub async fn gen_filled_store(
        amount: u64,
        path: Option<&Path>,
    ) -> (SledStore, ExtendedHeaderGenerator) {
        let s = if let Some(path) = path {
            SledStore::new_in_path(path).await.unwrap()
        } else {
            SledStore::new_temp().await.unwrap()
        };

        let mut gen = ExtendedHeaderGenerator::new();

        let headers = gen.next_many(amount);

        for header in headers {
            s.append_single_unchecked(header)
                .await
                .expect("inserting test data failed");
        }

        (s, gen)
    }
}
