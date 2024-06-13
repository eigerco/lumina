use std::ops::RangeInclusive;
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
use tracing::{debug, trace};

use crate::store::header_ranges::{
    HeaderRange, HeaderRanges, HeaderRangesExt, VerifiedExtendedHeaders,
};
use crate::store::utils::RangeScanResult;
use crate::store::{Result, SamplingMetadata, SamplingStatus, Store, StoreError};

const SCHEMA_VERSION: u64 = 1;

const HEADER_HEIGHT_RANGES: TableDefinition<'static, u64, (u64, u64)> =
    TableDefinition::new("STORE.HEIGHT_RANGES");
const HEIGHTS_TABLE: TableDefinition<'static, &[u8], u64> = TableDefinition::new("STORE.HEIGHTS");
const HEADERS_TABLE: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("STORE.HEADERS");
const SAMPLING_METADATA_TABLE: TableDefinition<'static, u64, &[u8]> =
    TableDefinition::new("STORE.SAMPLING_METADATA");
const SCHEMA_VERSION_TABLE: TableDefinition<'static, (), u64> =
    TableDefinition::new("STORE.SCHEMA_VERSION");

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
                        // TODO: When we update the schema we need to perform manual migration
                        if schema_version != SCHEMA_VERSION {
                            let e = format!("Incompatible database schema; found {schema_version}, expected {SCHEMA_VERSION}.");
                            return Err(StoreError::OpenFailed(e));
                        }
                    }
                    None => {
                        schema_version_table.insert((), SCHEMA_VERSION)?;
                    }
                }

                // create tables, so that reads later don't complain
                let _heights_table = tx.open_table(HEIGHTS_TABLE)?;
                let _ranges_table = tx.open_table(HEADER_HEIGHT_RANGES)?;

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
            let table = tx.open_table(HEADER_HEIGHT_RANGES)?;
            let highest_range = get_head_range(&table)?;

            if highest_range.is_empty() {
                Err(StoreError::NotFound)
            } else {
                Ok(*highest_range.end())
            }
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
            let height_ranges_table = tx.open_table(HEADER_HEIGHT_RANGES)?;
            let headers_table = tx.open_table(HEADERS_TABLE)?;

            let head_range = get_head_range(&height_ranges_table)?;
            get_header(&headers_table, *head_range.end())
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

    async fn insert_verified_headers(&self, headers: VerifiedExtendedHeaders) -> Result<()> {
        self.write_tx(move |tx| {
            let headers = headers.as_ref();
            let (Some(head), Some(tail)) = (headers.first(), headers.last()) else {
                return Ok(());
            };
            let mut heights_table = tx.open_table(HEIGHTS_TABLE)?;
            let mut headers_table = tx.open_table(HEADERS_TABLE)?;
            let mut height_ranges_table = tx.open_table(HEADER_HEIGHT_RANGES)?;

            let headers_range = head.height().value()..=tail.height().value();
            let (prev_exists, next_exists) =
                try_insert_to_range(&mut height_ranges_table, headers_range)?;

            verify_against_neighbours(
                &headers_table,
                prev_exists.then_some(head),
                next_exists.then_some(tail),
            )?;

            for header in headers {
                let height = header.height().value();
                // until unwrap_infallible is stabilised, make sure Result is Infallible manually
                let serialized_header: Result<_, Infallible> = header.encode_vec();
                let serialized_header = serialized_header.unwrap();

                if headers_table
                    .insert(height, &serialized_header[..])?
                    .is_some()
                {
                    return Err(StoreError::StoredDataError(
                        "inconsistency between headers and ranges table".into(),
                    ));
                }

                let hash = header.hash();
                if heights_table.insert(hash.as_bytes(), height)?.is_some() {
                    return Err(StoreError::HashExists(hash));
                }

                trace!("Inserted header {hash} with height {height}");
            }
            debug!(
                "Inserted header range {:?}",
                head.height().value()..=tail.height().value()
            );
            Ok(())
        })
        .await?;

        self.inner.header_added_notifier.notify_waiters();

        Ok(())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        self.write_tx(move |tx| {
            let mut sampling_metadata_table = tx.open_table(SAMPLING_METADATA_TABLE)?;
            let ranges_table = tx.open_table(HEADER_HEIGHT_RANGES)?;
            if !get_all_ranges(&ranges_table)?.contains(height) {
                return Err(StoreError::NotFound);
            }

            let previous = get_sampling_metadata(&sampling_metadata_table, height)?;

            let entry = match previous {
                Some(mut previous) => {
                    previous.status = status;

                    for cid in cids {
                        if !previous.cids.contains(&cid) {
                            previous.cids.push(cid);
                        }
                    }

                    previous
                }
                None => SamplingMetadata { status, cids },
            };

            // make sure Result is Infallible and unwrap it later
            let serialized: Result<_, Infallible> = entry.encode_vec();
            let serialized = serialized.unwrap();

            sampling_metadata_table.insert(height, &serialized[..])?;

            Ok(())
        })
        .await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.read_tx(move |tx| {
            let headers_table = tx.open_table(HEADERS_TABLE)?;
            let sampling_metadata_table = tx.open_table(SAMPLING_METADATA_TABLE)?;

            if headers_table.get(height)?.is_none() {
                return Err(StoreError::NotFound);
            }

            get_sampling_metadata(&sampling_metadata_table, height)
        })
        .await
    }

    async fn get_stored_ranges(&self) -> Result<HeaderRanges> {
        let ranges = self
            .read_tx(|tx| {
                let table = tx.open_table(HEADER_HEIGHT_RANGES)?;
                get_all_ranges(&table)
            })
            .await?;

        Ok(ranges)
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

    async fn wait_new_head(&self) -> u64 {
        let head = self.head_height().await.unwrap_or(0);
        let mut notifier = pin!(self.inner.header_added_notifier.notified());

        loop {
            let new_head = self.head_height().await.unwrap_or(0);

            if head != new_head {
                return new_head;
            }

            // Await for a notification
            notifier.as_mut().await;

            // Reset notifier
            notifier.set(self.inner.header_added_notifier.notified());
        }
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

    async fn insert_verified_headers(&self, headers: VerifiedExtendedHeaders) -> Result<()> {
        self.insert_verified_headers(headers).await
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        self.update_sampling_metadata(height, status, cids).await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        self.get_sampling_metadata(height).await
    }

    async fn get_stored_header_ranges(&self) -> Result<HeaderRanges> {
        Ok(self.get_stored_ranges().await?)
    }
}

fn try_insert_to_range(
    ranges_table: &mut Table<u64, (u64, u64)>,
    new_range: HeaderRange,
) -> Result<(bool, bool)> {
    let stored_ranges = HeaderRanges::from_vec(
        ranges_table
            .iter()?
            .map(|range_guard| {
                let range = range_guard?.1.value();
                Ok(range.0..=range.1)
            })
            .collect::<Result<_>>()?,
    );

    let RangeScanResult {
        range_index,
        range,
        range_to_remove,
    } = stored_ranges.check_range_insert(&new_range)?;

    if let Some(to_remove) = range_to_remove {
        let (start, end) = ranges_table
            .remove(u64::try_from(to_remove).expect("usize->u64"))?
            .expect("missing range")
            .value();

        debug!("consolidating range, new range: {range:?}, removed {start}..={end}");
    };
    let prev_exists = new_range.start() != range.start();
    let next_exists = new_range.end() != range.end();

    ranges_table.insert(
        u64::try_from(range_index).expect("usize->u64"),
        (*range.start(), *range.end()),
    )?;

    Ok((prev_exists, next_exists))
}

fn verify_against_neighbours<R>(
    headers_table: &R,
    lowest_header: Option<&ExtendedHeader>,
    highest_header: Option<&ExtendedHeader>,
) -> Result<()>
where
    R: ReadableTable<u64, &'static [u8]>,
{
    if let Some(lowest_header) = lowest_header {
        let prev = get_header(headers_table, lowest_header.height().value() - 1).map_err(|e| {
            if let StoreError::NotFound = e {
                StoreError::StoredDataError("inconsistency between headers and ranges table".into())
            } else {
                e
            }
        })?;
        prev.verify(lowest_header)?;
    }

    if let Some(highest_header) = highest_header {
        let next = get_header(headers_table, highest_header.height().value() + 1).map_err(|e| {
            if let StoreError::NotFound = e {
                StoreError::StoredDataError("inconsistency between headers and ranges table".into())
            } else {
                e
            }
        })?;
        highest_header.verify(&next)?;
    }

    Ok(())
}

fn get_head_range<R>(ranges_table: &R) -> Result<RangeInclusive<u64>>
where
    R: ReadableTable<u64, (u64, u64)>,
{
    ranges_table
        .last()?
        .map(|(_key_guard, value_guard)| {
            let range = value_guard.value();
            range.0..=range.1
        })
        .ok_or(StoreError::NotFound)
}

fn get_all_ranges<R>(ranges_table: &R) -> Result<HeaderRanges>
where
    R: ReadableTable<u64, (u64, u64)>,
{
    Ok(HeaderRanges::from_vec(
        ranges_table
            .iter()?
            .map(|range_guard| {
                let range = range_guard?.1.value();
                Ok(range.0..=range.1)
            })
            .collect::<Result<_>>()?,
    ))
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
    use crate::store::ExtendedHeaderGeneratorExt;

    use super::*;

    use std::path::Path;

    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_store_persistence() {
        let db_dir = TempDir::with_prefix("lumina.store.test").unwrap();
        let db = db_dir.path().join("db");

        let (original_store, mut gen) = gen_filled_store(0, Some(&db)).await;
        let mut original_headers = gen.next_many(20);

        original_store
            .insert(original_headers.clone())
            .await
            .expect("inserting test data failed");
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
        reopened_store
            .insert(new_headers.clone())
            .await
            .expect("failed to insert data");
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
        store0.insert(headers.clone()).await.unwrap();
        store1.insert(headers).await.unwrap();

        let mut gen1 = gen0.fork();

        store0.insert(gen0.next_many_verified(5)).await.unwrap();
        store1.insert(gen1.next_many_verified(6)).await.unwrap();

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

        s.insert(headers).await.expect("inserting test data failed");

        (s, gen)
    }
}
