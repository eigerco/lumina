use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use sled::transaction::{abort, ConflictableTransactionError, TransactionError};
use sled::{Db, Error as SledError, Transactional, Tree};
use tokio::task::spawn_blocking;
use tokio::task::JoinError;
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
                        return abort(StoreError::HeightExists(height));
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

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            // Make sure we have the header being marked
            let head_height = read_height_by_db_key(&inner.db, HEAD_HEIGHT_KEY)?;
            if height == 0 || height > head_height {
                return Err(StoreError::NotFound);
            }

            let metadata_key = height_to_key(height);

            let new_inserted = &inner
                .sampling_metadata
                .transaction(move |sampling_metadata| {
                    let previous: Option<SamplingMetadata> = sampling_metadata
                        .get(metadata_key)?
                        .as_deref()
                        .map(Protobuf::decode_vec)
                        .transpose()
                        .map_err(|e| {
                            ConflictableTransactionError::Abort(StoreError::StoredDataError(
                                e.to_string(),
                            ))
                        })?;
                    let update_sampling_height = previous.is_none();

                    let entry = match previous {
                        Some(mut previous) => {
                            previous.accepted = accepted;
                            previous.cids_sampled.extend_from_slice(&cids);
                            previous
                        }
                        None => SamplingMetadata {
                            accepted,
                            cids_sampled: cids.clone(),
                        },
                    };

                    let serialized: Result<_, Infallible> = entry.encode_vec();
                    sampling_metadata.insert(&metadata_key, serialized.unwrap())?;

                    Ok(update_sampling_height)
                })?;

            if *new_inserted {
                // sled does not have contains_key for TransactionalTree, so to scan the db
                // for new height, we'd have to actually pull the data out.  Instead, update
                // the height outside of txn and manually take care of synchronisation.
                update_cached_sampling_height(&inner.db, &inner.sampling_metadata)
            } else {
                info!("Overriding existing sampling metadata for height {height}");
                read_height_by_db_key(&inner.db, NEXT_UNSAMPLED_HEIGHT_KEY)
            }
        })
        .await?
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let metadata_key = height_to_key(height);
            let Some(serialized) = inner.sampling_metadata.get(metadata_key)? else {
                return Ok(None);
            };

            let metadata = SamplingMetadata::decode_vec(serialized.as_ref())
                .map_err(|e| StoreError::StoredDataError(e.to_string()))?;

            Ok(Some(metadata))
        })
        .await?
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
fn update_cached_sampling_height(db: &Tree, sampling_metadata: &Tree) -> Result<u64> {
    loop {
        let previous_height = read_height_by_db_key(db, NEXT_UNSAMPLED_HEIGHT_KEY)?;
        let mut current_height = previous_height;
        while sampling_metadata.contains_key(height_to_key(current_height))? {
            current_height += 1;
        }

        if db
            .compare_and_swap(
                NEXT_UNSAMPLED_HEIGHT_KEY,
                Some(&height_to_key(previous_height)),
                Some(&height_to_key(current_height)),
            )?
            .is_ok()
        {
            break Ok(current_height);
        }
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

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_contains_height() {
        let s = gen_filled_store(2, None).await.0;

        assert!(!s.has_at(0).await);
        assert!(s.has_at(1).await);
        assert!(s.has_at(2).await);
        assert!(!s.has_at(3).await);
    }

    #[tokio::test]
    async fn test_empty_store() {
        let s = create_store(None).await;
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
        let s = create_store(None).await;
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

        let s = create_store(None).await;

        assert!(matches!(
            s.append_single_unchecked(header5).await,
            Err(StoreError::NonContinuousAppend(0, 5))
        ));
    }

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

    #[tokio::test]
    async fn test_sampling_height_empty_store() {
        let (store, _) = gen_filled_store(0, None).await;
        store
            .update_sampling_metadata(0, true, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(1, true, vec![])
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn test_sampling_height() {
        let (store, _) = gen_filled_store(9, None).await;

        store
            .update_sampling_metadata(0, true, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(1, true, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(2, true, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(3, false, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(4, true, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(5, false, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(6, false, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(8, true, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(10, true, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(10, false, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(20, true, vec![])
            .await
            .unwrap_err();

        assert_eq!(store.next_unsampled_height().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn test_sampling_merge() {
        let (store, _) = gen_filled_store(1, None).await;
        let cid0 = "zdpuAyvkgEDQm9TenwGkd5eNaosSxjgEYd8QatfPetgB1CdEZ"
            .parse()
            .unwrap();
        let cid1 = "zb2rhe5P4gXftAwvA4eXQ5HJwsER2owDyS9sKaQRRVQPn93bA"
            .parse()
            .unwrap();

        store
            .update_sampling_metadata(1, false, vec![cid0])
            .await
            .unwrap();
        store
            .update_sampling_metadata(1, false, vec![])
            .await
            .unwrap();

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert!(!sampling_data.accepted);
        assert_eq!(sampling_data.cids_sampled, vec![cid0]);

        store
            .update_sampling_metadata(1, true, vec![cid1])
            .await
            .unwrap();

        assert_eq!(store.next_unsampled_height().await.unwrap(), 2);

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert!(sampling_data.accepted);
        assert_eq!(sampling_data.cids_sampled, vec![cid0, cid1]);
    }

    #[tokio::test]
    async fn test_sampled_cids() {
        let (store, _) = gen_filled_store(5, None).await;

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
            .await
            .unwrap();
        store
            .update_sampling_metadata(2, true, cids[0..1].to_vec())
            .await
            .unwrap();
        store
            .update_sampling_metadata(4, false, cids[3..].to_vec())
            .await
            .unwrap();
        store
            .update_sampling_metadata(5, false, vec![])
            .await
            .unwrap();

        assert_eq!(store.next_unsampled_height().await.unwrap(), 3);

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids);
        assert!(sampling_data.accepted);

        let sampling_data = store.get_sampling_metadata(2).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids[0..1]);
        assert!(sampling_data.accepted);

        assert!(store.get_sampling_metadata(3).await.unwrap().is_none());

        let sampling_data = store.get_sampling_metadata(4).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, cids[3..]);
        assert!(!sampling_data.accepted);

        let sampling_data = store.get_sampling_metadata(5).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids_sampled, vec![]);
        assert!(!sampling_data.accepted);
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
