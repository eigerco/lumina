use std::pin::pin;
use std::sync::Arc;
use std::{convert::Infallible, path::Path};

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use redb::{
    CommitError, Database, ReadTransaction, ReadableTable, StorageError, Table, TableDefinition,
    TableError, TransactionError, WriteTransaction,
};
use tokio::sync::Notify;
use tokio::task::spawn_blocking;
use tracing::{debug, info};

use crate::store::{Result, SamplingMetadata, Store, StoreError};

const SCEMA_VERSION: u64 = 1;

const HEAD_HEIGHT_KEY: &[u8] = b"KEY.HEAD_HEIGHT";
const NEXT_UNSAMPLED_HEIGHT_KEY: &[u8] = b"KEY.UNSAMPLED_HEIGHT";

const HEIGHTS_TABLE: TableDefinition<'static, &[u8], u64> = TableDefinition::new("STORE.HEIGHTS");
const HEADERS_TABLE: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("STORE.HEADERS");
const SAMPLING_METADATA_TABLE: TableDefinition<'static, u64, &[u8]> =
    TableDefinition::new("STORE.SAMPLING_METADATA");
const SCHEMA_VERSION_TABLE: TableDefinition<'static, (), u64> =
    TableDefinition::new("STORE.SCEMA_VERSION");

/// A [`Store`] implementation based on a [`redb`] database.
#[derive(Debug)]
pub struct RedbStore {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Reference to the entire redb database
    db: Arc<Database>,
    /// Notify when a new header is added
    header_added_notifier: Notify,
}

impl RedbStore {
    /// Open a persistent [`redb`] store.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_owned();

        let db = spawn_blocking(|| Database::create(path))
            .await?
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        RedbStore::new(Arc::new(db)).await
    }

    /// Open an in memory [`redb`] store.
    pub async fn in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        RedbStore::new(Arc::new(db)).await
    }

    /// Create new `RedbStore` with an already opened [`redb::Database`].
    pub async fn new(db: Arc<Database>) -> Result<Self> {
        let store = RedbStore {
            inner: Arc::new(Inner {
                db,
                header_added_notifier: Notify::new(),
            }),
        };

        store
            .write_tx(|tx| {
                let mut schema_version_table = tx.open_table(SCHEMA_VERSION_TABLE)?;
                let schema_version = schema_version_table.get(())?.map(|guard| guard.value());

                match schema_version {
                    Some(schema_version) => {
                        // TODO: When we schema we should provide migrate from older versions to
                        // newer ones.
                        if schema_version != SCEMA_VERSION {
                            let e = format!("Incompatible database schema; found {schema_version}, expected {SCEMA_VERSION}.");
                            return Err(StoreError::OpenFailed(e));
                        }
                    }
                    None => {
                        schema_version_table.insert((), SCEMA_VERSION)?;
                    }
                }

                let mut heights_table = tx.open_table(HEIGHTS_TABLE)?;

                if heights_table.get(HEAD_HEIGHT_KEY)?.is_none() {
                    heights_table.insert(HEAD_HEIGHT_KEY, 0)?;
                }

                if heights_table.get(NEXT_UNSAMPLED_HEIGHT_KEY)?.is_none() {
                    heights_table.insert(NEXT_UNSAMPLED_HEIGHT_KEY, 1)?;
                }

                Ok(())
            })
            .await
            .map_err(|e| match e {
                e @ StoreError::OpenFailed(_) => e,
                e => StoreError::OpenFailed(e.to_string()),
            })?;

        Ok(store)
    }

    /// Returns the raw [`redb::Database`].
    ///
    /// This is useful if you want to pass the database handle to any other
    /// stores (e.g. [`blockstore`]).
    pub fn raw_db(&self) -> Arc<Database> {
        self.inner.db.clone()
    }

    /// Execute a read transaction.
    async fn read_tx<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut ReadTransaction) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let mut tx = inner.db.begin_read()?;
            f(&mut tx)
        })
        .await?
    }

    /// Execute a write transaction.
    ///
    /// If closure returns an error the transaction is aborted, otherwise commited.
    async fn write_tx<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut WriteTransaction) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let mut tx = inner.db.begin_write()?;
            let res = f(&mut tx);

            if res.is_ok() {
                tx.commit()?;
            } else {
                tx.abort()?;
            }

            res
        })
        .await?
    }

    async fn head_height(&self) -> Result<u64> {
        self.read_tx(|tx| {
            let table = tx.open_table(HEIGHTS_TABLE)?;
            let head_height = get_height(&table, HEAD_HEIGHT_KEY)?;

            if head_height == 0 {
                Err(StoreError::NotFound)
            } else {
                Ok(head_height)
            }
        })
        .await
    }

    async fn get_next_unsampled_height(&self) -> Result<u64> {
        self.read_tx(|tx| {
            let table = tx.open_table(HEIGHTS_TABLE)?;
            get_height(&table, NEXT_UNSAMPLED_HEIGHT_KEY)
        })
        .await
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        let hash = *hash;

        self.read_tx(move |tx| {
            let heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let headers_table = tx.open_table(HEADERS_TABLE)?;

            let height = get_height(&heights_table, hash.as_bytes())?;
            get_header(&headers_table, height)
        })
        .await
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.read_tx(move |tx| {
            let table = tx.open_table(HEADERS_TABLE)?;
            get_header(&table, height)
        })
        .await
    }

    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.read_tx(|tx| {
            let heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let headers_table = tx.open_table(HEADERS_TABLE)?;

            let head_height = get_height(&heights_table, HEAD_HEIGHT_KEY)?;
            get_header(&headers_table, head_height)
        })
        .await
    }

    async fn contains_hash(&self, hash: &Hash) -> bool {
        let hash = *hash;

        self.read_tx(move |tx| {
            let heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let headers_table = tx.open_table(HEADERS_TABLE)?;

            let height = get_height(&heights_table, hash.as_bytes())?;
            Ok(headers_table.get(height)?.is_some())
        })
        .await
        .unwrap_or(false)
    }

    async fn contains_height(&self, height: u64) -> bool {
        self.read_tx(move |tx| {
            let headers_table = tx.open_table(HEADERS_TABLE)?;
            Ok(headers_table.get(height)?.is_some())
        })
        .await
        .unwrap_or(false)
    }

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        self.write_tx(move |tx| {
            let mut heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let mut headers_table = tx.open_table(HEADERS_TABLE)?;

            let hash = header.hash();
            let height = header.height().value();
            let head_height = get_height(&heights_table, HEAD_HEIGHT_KEY)?;

            // A light check before checking the whole map
            if head_height > 0 && height <= head_height {
                return Err(StoreError::HeightExists(height));
            }

            // Check if it's continuous before checking the whole map.
            if head_height + 1 != height {
                return Err(StoreError::NonContinuousAppend(head_height, height));
            }

            // make sure Result is Infallible and unwrap it later
            let serialized_header: Result<_, Infallible> = header.encode_vec();
            let serialized_header = serialized_header.unwrap();

            if headers_table
                .insert(height, &serialized_header[..])?
                .is_some()
            {
                return Err(StoreError::HeightExists(height));
            }

            if heights_table.insert(hash.as_bytes(), height)?.is_some() {
                return Err(StoreError::HashExists(hash));
            }

            heights_table.insert(HEAD_HEIGHT_KEY, height)?;

            debug!("Inserted header {hash} with height {height}");
            Ok(())
        })
        .await?;

        self.inner.header_added_notifier.notify_waiters();

        Ok(())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        self.write_tx(move |tx| {
            let mut heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let mut sampling_metadata_table = tx.open_table(SAMPLING_METADATA_TABLE)?;

            // Make sure we have the header being marked
            let head_height = get_height(&heights_table, HEAD_HEIGHT_KEY)?;

            if height == 0 || height > head_height {
                return Err(StoreError::NotFound);
            }

            let previous = get_sampling_metadata(&sampling_metadata_table, height)?;
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

            // make sure Result is Infallible and unwrap it later
            let serialized: Result<_, Infallible> = entry.encode_vec();
            let serialized = serialized.unwrap();

            sampling_metadata_table.insert(height, &serialized[..])?;

            if new_inserted {
                update_sampling_height(&mut heights_table, &mut sampling_metadata_table)
            } else {
                info!("Overriding existing sampling metadata for height {height}");
                get_height(&heights_table, NEXT_UNSAMPLED_HEIGHT_KEY)
            }
        })
        .await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.read_tx(move |tx| {
            let heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let sampling_metadata_table = tx.open_table(SAMPLING_METADATA_TABLE)?;

            // Make sure we have the header of height
            let head_height = get_height(&heights_table, HEAD_HEIGHT_KEY)?;

            if height == 0 || height > head_height {
                return Err(StoreError::NotFound);
            }

            get_sampling_metadata(&sampling_metadata_table, height)
        })
        .await
    }
}

#[async_trait]
impl Store for RedbStore {
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
fn get_height<R>(heights_table: &R, key: &[u8]) -> Result<u64>
where
    R: ReadableTable<&'static [u8], u64>,
{
    heights_table
        .get(key)?
        .map(|guard| guard.value())
        .ok_or(StoreError::NotFound)
}

#[inline]
fn get_header<R>(headers_table: &R, key: u64) -> Result<ExtendedHeader>
where
    R: ReadableTable<u64, &'static [u8]>,
{
    let serialized = headers_table.get(key)?.ok_or(StoreError::NotFound)?;
    ExtendedHeader::decode(serialized.value()).map_err(|e| StoreError::CelestiaTypes(e.into()))
}

#[inline]
fn get_sampling_metadata<R>(
    sampling_metadata_table: &R,
    key: u64,
) -> Result<Option<SamplingMetadata>>
where
    R: ReadableTable<u64, &'static [u8]>,
{
    sampling_metadata_table
        .get(key)?
        .map(|guard| {
            SamplingMetadata::decode(guard.value())
                .map_err(|e| StoreError::StoredDataError(e.to_string()))
        })
        .transpose()
}

#[inline]
fn update_sampling_height(
    heights_table: &mut Table<&'static [u8], u64>,
    sampling_metadata_table: &mut Table<u64, &'static [u8]>,
) -> Result<u64> {
    let previous_height = get_height(heights_table, NEXT_UNSAMPLED_HEIGHT_KEY)?;
    let mut new_height = previous_height;

    while sampling_metadata_table.get(new_height)?.is_some() {
        new_height += 1;
    }

    heights_table.insert(NEXT_UNSAMPLED_HEIGHT_KEY, new_height)?;

    Ok(new_height)
}

impl From<TransactionError> for StoreError {
    fn from(e: TransactionError) -> Self {
        match e {
            TransactionError::ReadTransactionStillInUse(_) => {
                unreachable!("redb::ReadTransaction::close is never used")
            }
            e => StoreError::FatalDatabaseError(e.to_string()),
        }
    }
}

impl From<TableError> for StoreError {
    fn from(e: TableError) -> Self {
        match e {
            TableError::Storage(e) => e.into(),
            TableError::TableAlreadyOpen(table, location) => {
                panic!("Table {table} already opened from: {location}")
            }
            TableError::TableDoesNotExist(_) => StoreError::NotFound,
            e => StoreError::StoredDataError(e.to_string()),
        }
    }
}

impl From<StorageError> for StoreError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::ValueTooLarge(_) => {
                unreachable!("redb::Table::insert_reserve is never used")
            }
            e => StoreError::FatalDatabaseError(e.to_string()),
        }
    }
}

impl From<CommitError> for StoreError {
    fn from(e: CommitError) -> Self {
        StoreError::FatalDatabaseError(e.to_string())
    }
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
        let db = db_dir.path().join("db");

        let (original_store, mut gen) = gen_filled_store(0, Some(&db)).await;
        let mut original_headers = gen.next_many(20);

        for h in &original_headers {
            original_store
                .append_single_unchecked(h.clone())
                .await
                .expect("inserting test data failed");
        }
        drop(original_store);

        let reopened_store = create_store(Some(&db)).await;

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

        let reopened_store = create_store(Some(&db)).await;
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

    pub async fn create_store(path: Option<&Path>) -> RedbStore {
        match path {
            Some(path) => RedbStore::open(path).await.unwrap(),
            None => RedbStore::in_memory().await.unwrap(),
        }
    }

    pub async fn gen_filled_store(
        amount: u64,
        path: Option<&Path>,
    ) -> (RedbStore, ExtendedHeaderGenerator) {
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
