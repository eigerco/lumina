use std::convert::Infallible;
use std::ops::Deref;
use std::pin::pin;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use sled::transaction::{abort, ConflictableTransactionError, TransactionError, TransactionalTree};
use sled::{Db, Error as SledError, Transactional, Tree};
use tokio::sync::Notify;
use tokio::task::spawn_blocking;
use tracing::{debug, info};

use crate::store::{Result, SamplingMetadata, Store, StoreError};

const HEAD_HEIGHT_KEY: &[u8] = b"KEY.HEAD_HEIGHT";
const NEXT_UNSAMPLED_HEIGHT_KEY: &[u8] = b"KEY.UNSAMPLED_HEIGHT";
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
    sampling_metadata: Tree,
    /// Notify when a new header is added
    header_added_notifier: Notify,
}

impl SledStore {
    /// Create or open a persistent store.
    pub async fn new(db: Db) -> Result<Self> {
        spawn_blocking(move || {
            let headers = db.open_tree(HASH_TREE_ID)?;
            let height_to_hash = db.open_tree(HEIGHT_TO_HASH_TREE_ID)?;
            let sampling_metadata = db.open_tree(HEIGHT_TO_METADATA_TREE_ID)?;

            if db
                .compare_and_swap(
                    NEXT_UNSAMPLED_HEIGHT_KEY,
                    None as Option<&[u8]>,
                    Some(&height_to_key(1)),
                )?
                .is_ok()
            {
                debug!("initialised sampling height");
            }

            Ok::<_, SledError>(Self {
                inner: Arc::new(Inner {
                    db,
                    headers,
                    height_to_hash,
                    sampling_metadata,
                    header_added_notifier: Notify::new(),
                }),
            })
        })
        .await?
        .map_err(|e| StoreError::OpenFailed(e.to_string()))
    }

    async fn head_height(&self) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY)).await?
    }

    async fn get_next_unsampled_height(&self) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || read_height_by_db_key(&inner.db, NEXT_UNSAMPLED_HEIGHT_KEY)).await?
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();
        let hash = *hash;

        spawn_blocking(move || read_header_by_db_key(&inner.headers, hash.as_bytes())).await?
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();

        Ok(spawn_blocking(move || {
            (&inner.headers, &inner.height_to_hash).transaction(move |(headers, height_to_hash)| {
                let hash =
                    transactional_read_hash_by_db_key(height_to_hash, &height_to_key(height))?;
                transactional_read_header_by_db_key(headers, hash.as_bytes())
            })
        })
        .await??)
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        let inner = self.inner.clone();

        Ok(spawn_blocking(move || {
            (inner.db.deref(), &inner.headers, &inner.height_to_hash).transaction(
                move |(db, headers, height_to_hash)| {
                    let head_height = transactional_read_height_by_db_key(db, HEAD_HEIGHT_KEY)?;

                    let hash = transactional_read_hash_by_db_key(
                        height_to_hash,
                        &height_to_key(head_height),
                    )?;

                    transactional_read_header_by_db_key(headers, hash.as_bytes())
                },
            )
        })
        .await??)
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
            // Do actual inserts as a transaction, failing if keys already exist
            (inner.db.deref(), &inner.headers, &inner.height_to_hash).transaction(
                move |(db, headers, height_to_hash)| {
                    let head_height =
                        transactional_read_height_by_db_key(db, HEAD_HEIGHT_KEY).unwrap_or(0);

                    // A light check before checking the whole map
                    if head_height > 0 && height <= head_height {
                        return abort(StoreError::HeightExists(height));
                    }

                    // Check if it's continuous before checking the whole map.
                    if head_height + 1 != height {
                        return abort(StoreError::NonContinuousAppend(head_height, height));
                    }

                    let height_key = height_to_key(height);
                    if height_to_hash
                        .insert(&height_key, hash.as_bytes())?
                        .is_some()
                    {
                        return abort(StoreError::HeightExists(height));
                    }

                    db.insert(HEAD_HEIGHT_KEY, &height_key)?;

                    // make sure Result is Infallible, we unwrap it later
                    let serialized_header: std::result::Result<_, Infallible> = header.encode_vec();

                    if headers
                        .insert(hash.as_bytes(), serialized_header.unwrap())?
                        .is_some()
                    {
                        return abort(StoreError::HashExists(hash));
                    }

                    Ok(())
                },
            )
        })
        .await??;

        self.inner.header_added_notifier.notify_waiters();

        debug!("Inserting header {hash} with height {height}");
        Ok(())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        let inner = self.inner.clone();

        Ok(spawn_blocking(move || {
            (inner.db.deref(), &inner.sampling_metadata).transaction(
                move |(db, sampling_metadata)| {
                    // Make sure we have the header being marked
                    let head_height = transactional_read_height_by_db_key(db, HEAD_HEIGHT_KEY)?;

                    if height == 0 || height > head_height {
                        return abort(StoreError::NotFound);
                    }

                    let metadata_key = height_to_key(height);

                    let previous = match transactional_read_sampling_metadata_by_db_key(
                        sampling_metadata,
                        &metadata_key,
                    ) {
                        Ok(val) => Some(val),
                        Err(ConflictableTransactionError::Abort(StoreError::NotFound)) => None,
                        Err(e) => return Err(e),
                    };
                    let new_inserted = previous.is_none();

                    let entry = match previous {
                        Some(mut previous) => {
                            previous.accepted = accepted;

                            for cid in &cids {
                                if !previous.cids_sampled.contains(cid) {
                                    previous.cids_sampled.push(cid.to_owned());
                                }
                            }

                            previous
                        }
                        None => SamplingMetadata {
                            accepted,
                            cids_sampled: cids.clone(),
                        },
                    };

                    let serialized: Result<_, Infallible> = entry.encode_vec();
                    sampling_metadata.insert(&metadata_key, serialized.unwrap())?;

                    if new_inserted {
                        transactional_update_sampling_height(db, sampling_metadata)
                    } else {
                        info!("Overriding existing sampling metadata for height {height}");
                        transactional_read_height_by_db_key(db, NEXT_UNSAMPLED_HEIGHT_KEY)
                    }
                },
            )
        })
        .await??)
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        let inner = self.inner.clone();

        Ok(spawn_blocking(move || {
            (inner.db.deref(), &inner.sampling_metadata).transaction(
                move |(db, sampling_metadata)| {
                    // Make sure we have the header of height
                    let head_height = transactional_read_height_by_db_key(db, HEAD_HEIGHT_KEY)?;

                    if height == 0 || height > head_height {
                        return abort(StoreError::NotFound);
                    }

                    let metadata_key = height_to_key(height);

                    match transactional_read_sampling_metadata_by_db_key(
                        sampling_metadata,
                        &metadata_key,
                    ) {
                        Ok(val) => Ok(Some(val)),
                        Err(ConflictableTransactionError::Abort(StoreError::NotFound)) => Ok(None),
                        Err(e) => Err(e),
                    }
                },
            )
        })
        .await??)
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

impl From<StoreError> for ConflictableTransactionError<StoreError> {
    fn from(value: StoreError) -> Self {
        ConflictableTransactionError::Abort(value)
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
            | e @ SledError::Io(_) => StoreError::FatalDatabaseError(e.to_string()),
        }
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

    async fn wait_height(&self, height: u64) -> Result<()> {
        let mut notifier = pin!(self.inner.header_added_notifier.notified());

        loop {
            if self.contains_height(height).await {
                return Ok(());
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.inner.header_added_notifier.notified());
        }
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

    async fn next_unsampled_height(&self) -> Result<u64> {
        self.get_next_unsampled_height().await
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        self.update_sampling_metadata(height, accepted, cids).await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height).await
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
fn transactional_read_height_by_db_key(
    tree: &TransactionalTree,
    db_key: &[u8],
) -> Result<u64, ConflictableTransactionError<StoreError>> {
    match tree
        .get(db_key)?
        .ok_or(StoreError::NotFound)?
        .as_ref()
        .try_into()
    {
        Ok(b) => Ok(u64::from_be_bytes(b)),
        Err(_) => abort(StoreError::NotFound),
    }
}

#[inline]
fn transactional_read_hash_by_db_key(
    tree: &TransactionalTree,
    db_key: &[u8],
) -> Result<Hash, ConflictableTransactionError<StoreError>> {
    match tree
        .get(db_key)?
        .ok_or(StoreError::NotFound)?
        .as_ref()
        .try_into()
    {
        Ok(b) => Ok(Hash::Sha256(b)),
        Err(_) => abort(StoreError::NotFound),
    }
}

#[inline]
fn transactional_update_sampling_height(
    db: &TransactionalTree,
    sampling_metadata: &TransactionalTree,
) -> Result<u64, ConflictableTransactionError<StoreError>> {
    let previous_height = transactional_read_height_by_db_key(db, NEXT_UNSAMPLED_HEIGHT_KEY)?;
    let mut current_height = previous_height;

    while sampling_metadata
        .get(height_to_key(current_height))?
        .is_some()
    {
        current_height += 1;
    }

    db.insert(NEXT_UNSAMPLED_HEIGHT_KEY, &height_to_key(current_height))?;

    Ok(current_height)
}

#[inline]
fn read_header_by_db_key(tree: &Tree, db_key: &[u8]) -> Result<ExtendedHeader> {
    let serialized = tree.get(db_key)?.ok_or(StoreError::NotFound)?;

    ExtendedHeader::decode(serialized.as_ref()).map_err(|e| StoreError::CelestiaTypes(e.into()))
}

#[inline]
fn transactional_read_header_by_db_key(
    tree: &TransactionalTree,
    db_key: &[u8],
) -> Result<ExtendedHeader, ConflictableTransactionError<StoreError>> {
    let serialized = tree.get(db_key)?.ok_or(StoreError::NotFound)?;

    Ok(ExtendedHeader::decode(serialized.as_ref())
        .map_err(|e| StoreError::CelestiaTypes(e.into()))?)
}

#[inline]
fn transactional_read_sampling_metadata_by_db_key(
    tree: &TransactionalTree,
    db_key: &[u8],
) -> Result<SamplingMetadata, ConflictableTransactionError<StoreError>> {
    let serialized = tree.get(db_key)?.ok_or(StoreError::NotFound)?;

    Ok(SamplingMetadata::decode(serialized.as_ref())
        .map_err(|e| StoreError::StoredDataError(e.to_string()))?)
}

#[inline]
fn height_to_key(height: u64) -> [u8; 8] {
    // sled recommends BigEndian representation for ints since it preserves expected int order
    // when sorted lexicographically
    height.to_be_bytes()
}

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_store_persistence() {
        let db_dir = TempDir::with_prefix("lumina.store.test").unwrap();
        let (original_store, mut gen) = gen_filled_store(0, Some(db_dir.path())).await;
        let mut original_headers = gen.next_many(20);

        for h in &original_headers {
            original_store
                .append_single_unchecked(h.clone())
                .await
                .expect("inserting test data failed");
        }
        drop(original_store);

        let reopened_store = create_store(Some(db_dir.path())).await;

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

        let reopened_store = create_store(Some(db_dir.path())).await;
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
        let store1 = create_store(None).await;

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

    mod migration_v1 {
        use super::*;
        use std::path::PathBuf;
        use tokio::time::{sleep, Duration};

        // Initialise a Store v1 (original design, only the extended headers and heights are
        // stored) and return a path to it.
        async fn init_store(hs: Vec<ExtendedHeader>) -> PathBuf {
            let tmp_path = TempDir::with_prefix("lumina.store.test")
                .unwrap()
                .into_path();
            let sled_path = tmp_path.clone();

            spawn_blocking(move || {
                let db = sled::Config::default()
                    .path(tmp_path)
                    .create_new(true) // make sure we fail if db is already there
                    .open()
                    .unwrap();

                let headers = db.open_tree(HASH_TREE_ID).unwrap();
                let height_to_hash = db.open_tree(HEIGHT_TO_HASH_TREE_ID).unwrap();

                // head height key is set only if there are headers present
                if let Some(head) = hs.last() {
                    db.insert(HEAD_HEIGHT_KEY, &height_to_key(head.height().value()))
                        .unwrap();
                }

                for header in hs {
                    let hash = header.hash();
                    let height = header.height().value();
                    let serialized_header = header.encode_vec().unwrap();

                    height_to_hash
                        .insert(height_to_key(height), hash.as_bytes())
                        .unwrap();
                    headers.insert(hash.as_bytes(), serialized_header).unwrap();
                }

                db.flush().unwrap();
            })
            .await
            .unwrap();

            // Add ~10ms wait so that any locks and opens properly close and unlock,
            // otherwise we might get spurious test failures
            // https://github.com/spacejam/sled/issues/1234#issuecomment-754769425
            sleep(Duration::from_millis(10)).await;

            sled_path
        }

        #[tokio::test]
        async fn migration_test() {
            let mut gen = ExtendedHeaderGenerator::new();
            let headers = gen.next_many(20);

            let v1_path = init_store(headers.clone()).await;

            let store = create_store(Some(&v1_path)).await;

            assert_eq!(store.head_height().await.unwrap(), 20);

            for header in headers {
                let height = header.height().value();

                let header_by_hash = store.get_by_hash(&header.hash()).await.unwrap();
                assert_eq!(header, header_by_hash);

                let header_by_height = store.get_by_height(height).await.unwrap();
                assert_eq!(header, header_by_height);

                // migrated headers should be marked as not sampled
                let sampling_data = store.get_sampling_metadata(height).await.unwrap();
                assert!(sampling_data.is_none());
            }

            store
                .update_sampling_metadata(1, true, vec![])
                .await
                .unwrap();
            let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
            assert!(sampling_data.accepted);
        }
    }

    pub async fn create_store(path: Option<&Path>) -> SledStore {
        let is_temp = path.is_none();
        let path = path.map(ToOwned::to_owned).unwrap_or_else(|| {
            TempDir::with_prefix("lumina.store.test")
                .unwrap()
                .into_path()
        });
        let db = spawn_blocking(move || {
            sled::Config::default()
                .path(path)
                .temporary(is_temp)
                .create_new(is_temp)
                .open()
                .unwrap()
        })
        .await
        .unwrap();
        SledStore::new(db).await.unwrap()
    }

    pub async fn gen_filled_store(
        amount: u64,
        path: Option<&Path>,
    ) -> (SledStore, ExtendedHeaderGenerator) {
        let s = create_store(path).await;

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
