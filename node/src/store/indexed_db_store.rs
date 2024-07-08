use std::cell::RefCell;
use std::convert::Infallible;
use std::pin::pin;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use rexie::{Direction, Index, KeyRange, ObjectStore, Rexie, TransactionMode};
use send_wrapper::SendWrapper;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::Notify;
use tracing::warn;
use wasm_bindgen::JsValue;

use crate::block_ranges::BlockRanges;
use crate::store::utils::VerifiedExtendedHeaders;
use crate::store::{Result, SamplingMetadata, SamplingStatus, Store, StoreError};

/// indexeddb version, needs to be incremented on every schema schange
const DB_VERSION: u32 = 4;

// Data stores (SQL table analogue) used in IndexedDb
const HEADER_STORE_NAME: &str = "headers";
const SAMPLING_STORE_NAME: &str = "sampling";
const RANGES_STORE_NAME: &str = "ranges";
const SCHEMA_STORE_NAME: &str = "schema";

// Additional indexes set on HEADER_STORE, for querying by height and hash
const HASH_INDEX_NAME: &str = "hash";
const HEIGHT_INDEX_NAME: &str = "height";

const ACCEPTED_SAMPLING_RANGES_KEY: &str = "accepted_sampling_ranges";
const HEADER_RANGES_KEY: &str = "header_ranges";
const VERSION_KEY: &str = "version";

#[derive(Debug, Serialize, Deserialize)]
struct ExtendedHeaderEntry {
    // We use those fields as indexes, names need to match ones in `add_index`
    height: u64,
    hash: Hash,
    header: Vec<u8>,
}

/// A [`Store`] implementation based on a `IndexedDB` browser database.
#[derive(Debug)]
pub struct IndexedDbStore {
    // SendWrapper usage is safe in wasm because we're running on a single thread
    head: SendWrapper<RefCell<Option<ExtendedHeader>>>,
    db: SendWrapper<Rexie>,
    header_added_notifier: Notify,
}

impl IndexedDbStore {
    /// Create or open a persistent store.
    pub async fn new(name: &str) -> Result<IndexedDbStore> {
        let rexie = Rexie::builder(name)
            .version(DB_VERSION)
            .add_object_store(
                ObjectStore::new(HEADER_STORE_NAME)
                    .key_path("id")
                    .auto_increment(true)
                    // These need to match names in `ExtendedHeaderEntry`
                    .add_index(Index::new(HASH_INDEX_NAME, "hash").unique(true))
                    .add_index(Index::new(HEIGHT_INDEX_NAME, "height").unique(true)),
            )
            .add_object_store(ObjectStore::new(RANGES_STORE_NAME))
            .add_object_store(ObjectStore::new(SAMPLING_STORE_NAME))
            .add_object_store(ObjectStore::new(SCHEMA_STORE_NAME))
            .build()
            .await
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        // NOTE: Rexie does not expose any migration functionality, so we
        // write our version in the store in order to handle it properly.
        match get_schema_version_from_db(&rexie).await? {
            Some(schema_version) => {
                if schema_version > DB_VERSION {
                    let e = format!(
                        "Incompatible database schema; found {}, expected {}.",
                        schema_version, DB_VERSION
                    );
                    return Err(StoreError::OpenFailed(e));
                }

                migrate_older_to_v4(&rexie).await?;
            }
            None => {
                // New database
                let tx = rexie.transaction(&[SCHEMA_STORE_NAME], TransactionMode::ReadWrite)?;

                let schema_store = tx.store(SCHEMA_STORE_NAME)?;
                set_schema_version(&schema_store, DB_VERSION).await?;

                tx.commit().await?;
            }
        }

        // Force us to write migrations!
        debug_assert_eq!(
            get_schema_version_from_db(&rexie).await?,
            Some(DB_VERSION),
            "Some migrations are missing"
        );

        let db_head = match get_head_from_database(&rexie).await {
            Ok(v) => Some(v),
            Err(StoreError::NotFound) => None,
            Err(e) => return Err(e),
        };

        Ok(IndexedDbStore {
            head: SendWrapper::new(RefCell::new(db_head)),
            db: SendWrapper::new(rexie),
            header_added_notifier: Notify::new(),
        })
    }

    /// Delete the persistent store.
    pub async fn delete_db(self) -> rexie::Result<()> {
        let name = self.db.name();
        self.db.take().close();
        Rexie::delete(&name).await
    }

    fn get_head(&self) -> Result<ExtendedHeader> {
        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        self.head.borrow().clone().ok_or(StoreError::NotFound)
    }

    fn get_head_height(&self) -> Result<u64> {
        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        self.head
            .borrow()
            .as_ref()
            .map(|h| h.height().value())
            .ok_or(StoreError::NotFound)
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let tx = self
            .db
            .transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;

        get_by_height(&tx.store(HEADER_STORE_NAME)?, height).await
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        let tx = self
            .db
            .transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;
        let header_store = tx.store(HEADER_STORE_NAME)?;
        let hash_index = header_store.index(HASH_INDEX_NAME)?;

        let hash_key = to_value(&hash)?;
        let header_entry = hash_index.get(&hash_key).await?;

        if header_entry.is_falsy() {
            return Err(StoreError::NotFound);
        }

        let serialized_header = from_value::<ExtendedHeaderEntry>(header_entry)?.header;
        ExtendedHeader::decode(serialized_header.as_ref())
            .map_err(|e| StoreError::CelestiaTypes(e.into()))
    }

    async fn get_stored_header_ranges(&self) -> Result<BlockRanges> {
        let tx = self
            .db
            .transaction(&[RANGES_STORE_NAME], TransactionMode::ReadOnly)?;
        let store = tx.store(RANGES_STORE_NAME)?;

        get_ranges(&store, HEADER_RANGES_KEY).await
    }

    async fn insert<R>(&self, headers: R) -> Result<()>
    where
        R: TryInto<VerifiedExtendedHeaders> + Send,
        StoreError: From<<R as TryInto<VerifiedExtendedHeaders>>::Error>,
    {
        let headers = headers.try_into()?;
        let (Some(head), Some(tail)) = (headers.as_ref().first(), headers.as_ref().last()) else {
            return Ok(());
        };

        let tx = self.db.transaction(
            &[HEADER_STORE_NAME, RANGES_STORE_NAME],
            TransactionMode::ReadWrite,
        )?;
        let header_store = tx.store(HEADER_STORE_NAME)?;
        let ranges_store = tx.store(RANGES_STORE_NAME)?;

        let mut header_ranges = get_ranges(&ranges_store, HEADER_RANGES_KEY).await?;

        let headers_range = head.height().value()..=tail.height().value();
        let (prev_exists, next_exists) =
            header_ranges.check_insertion_constrains(&headers_range)?;

        // header range is already internally verified against itself in `P2p::get_unverified_header_ranges`
        verify_against_neighbours(
            &header_store,
            prev_exists.then_some(head),
            next_exists.then_some(tail),
        )
        .await?;

        for header in headers.as_ref() {
            let hash = header.hash();
            let hash_index = header_store.index(HASH_INDEX_NAME)?;
            let jsvalue_hash_key = KeyRange::only(&to_value(&hash)?)?;
            if hash_index.count(Some(&jsvalue_hash_key)).await.unwrap_or(0) != 0 {
                return Err(StoreError::HashExists(hash));
            }

            // make sure Result is Infallible, we unwrap it later
            let serialized_header: std::result::Result<_, Infallible> = header.encode_vec();

            let height = header.height().value();
            let header_entry = ExtendedHeaderEntry {
                height,
                hash,
                header: serialized_header.unwrap(),
            };

            let jsvalue_header = to_value(&header_entry)?;

            header_store.add(&jsvalue_header, None).await?;
        }

        header_ranges.insert_relaxed(headers_range)?;
        set_ranges(&ranges_store, HEADER_RANGES_KEY, &header_ranges).await?;

        tx.commit().await?;

        if tail.height().value()
            > self
                .head
                .borrow()
                .as_ref()
                .map(|h| h.height().value())
                .unwrap_or(0)
        {
            self.head.replace(Some(tail.clone()));
        }

        self.header_added_notifier.notify_waiters();

        Ok(())
    }

    async fn contains_hash(&self, hash: &Hash) -> Result<bool> {
        let tx = self
            .db
            .transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;

        let header_store = tx.store(HEADER_STORE_NAME)?;
        let hash_index = header_store.index(HASH_INDEX_NAME)?;

        let hash_key = KeyRange::only(&to_value(&hash)?)?;

        let hash_count = hash_index.count(Some(&hash_key)).await?;

        Ok(hash_count > 0)
    }

    async fn contains_height(&self, height: u64) -> bool {
        let Ok(stored_ranges) = self.get_stored_header_ranges().await else {
            return false;
        };
        stored_ranges.contains(height)
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        let tx = self.db.transaction(
            &[SAMPLING_STORE_NAME, RANGES_STORE_NAME],
            TransactionMode::ReadWrite,
        )?;
        let sampling_store = tx.store(SAMPLING_STORE_NAME)?;
        let ranges_store = tx.store(RANGES_STORE_NAME)?;

        let header_ranges = get_ranges(&ranges_store, HEADER_RANGES_KEY).await?;
        let mut accepted_ranges = get_ranges(&ranges_store, ACCEPTED_SAMPLING_RANGES_KEY).await?;

        if !header_ranges.contains(height) {
            return Err(StoreError::NotFound);
        }

        let height_key = to_value(&height)?;
        let previous_entry = sampling_store.get(&height_key).await?;

        let new_entry = if previous_entry.is_falsy() {
            SamplingMetadata { status, cids }
        } else {
            let mut value: SamplingMetadata = from_value(previous_entry)?;

            value.status = status;

            for cid in cids {
                if !value.cids.contains(&cid) {
                    value.cids.push(cid);
                }
            }

            value
        };

        let metadata_jsvalue = to_value(&new_entry)?;
        sampling_store
            .put(&metadata_jsvalue, Some(&height_key))
            .await?;

        match status {
            SamplingStatus::Accepted => accepted_ranges
                .insert_relaxed(height..=height)
                .expect("invalid height"),
            _ => accepted_ranges
                .remove_relaxed(height..=height)
                .expect("invalid height"),
        }

        set_ranges(
            &ranges_store,
            ACCEPTED_SAMPLING_RANGES_KEY,
            &accepted_ranges,
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        if !self.contains_height(height).await {
            return Err(StoreError::NotFound);
        }

        let tx = self
            .db
            .transaction(&[SAMPLING_STORE_NAME], TransactionMode::ReadOnly)?;
        let store = tx.store(SAMPLING_STORE_NAME)?;

        let height_key = to_value(&height)?;
        let sampling_entry = store.get(&height_key).await?;

        if sampling_entry.is_falsy() {
            return Ok(None);
        }

        Ok(Some(from_value(sampling_entry)?))
    }

    async fn get_sampling_ranges(&self) -> Result<BlockRanges> {
        let tx = self
            .db
            .transaction(&[RANGES_STORE_NAME], TransactionMode::ReadWrite)?;
        let store = tx.store(RANGES_STORE_NAME)?;

        get_ranges(&store, ACCEPTED_SAMPLING_RANGES_KEY).await
    }
}

#[async_trait]
impl Store for IndexedDbStore {
    async fn get_head(&self) -> Result<ExtendedHeader> {
        self.get_head()
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        let fut = SendWrapper::new(self.get_by_hash(hash));
        fut.await
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        let fut = SendWrapper::new(self.get_by_height(height));
        fut.await
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
            let fut = SendWrapper::new(self.contains_height(height));
            if fut.await {
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
        let fut = SendWrapper::new(self.contains_hash(hash));
        fut.await.unwrap_or(false)
    }

    async fn has_at(&self, height: u64) -> bool {
        let fut = SendWrapper::new(self.contains_height(height));
        fut.await
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()> {
        let fut = SendWrapper::new(self.update_sampling_metadata(height, status, cids));
        fut.await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        let fut = SendWrapper::new(self.get_sampling_metadata(height));
        fut.await
    }

    async fn insert<R>(&self, header: R) -> Result<()>
    where
        R: TryInto<VerifiedExtendedHeaders> + Send,
        StoreError: From<<R as TryInto<VerifiedExtendedHeaders>>::Error>,
    {
        let fut = SendWrapper::new(self.insert(header));
        fut.await
    }

    async fn get_stored_header_ranges(&self) -> Result<BlockRanges> {
        let fut = SendWrapper::new(self.get_stored_header_ranges());
        fut.await
    }

    async fn get_accepted_sampling_ranges(&self) -> Result<BlockRanges> {
        let fut = SendWrapper::new(self.get_sampling_ranges());
        fut.await
    }
}

impl From<rexie::Error> for StoreError {
    fn from(error: rexie::Error) -> StoreError {
        use rexie::Error as E;
        match error {
            e @ E::AsyncChannelError => StoreError::ExecutorError(e.to_string()),
            other => StoreError::FatalDatabaseError(other.to_string()),
        }
    }
}

impl From<serde_wasm_bindgen::Error> for StoreError {
    fn from(error: serde_wasm_bindgen::Error) -> StoreError {
        StoreError::StoredDataError(format!("Error de/serializing: {error}"))
    }
}

async fn get_ranges(store: &rexie::Store, name: &str) -> Result<BlockRanges> {
    let key = JsValue::from_str(name);
    let raw_ranges = store.get(&key).await?;

    if raw_ranges.is_falsy() {
        // Ranges not set yet
        return Ok(BlockRanges::default());
    }

    Ok(from_value(raw_ranges)?)
}

async fn set_ranges(store: &rexie::Store, name: &str, ranges: &BlockRanges) -> Result<()> {
    let key = JsValue::from_str(name);
    let val = to_value(ranges)?;

    store.put(&val, Some(&key)).await?;

    Ok(())
}

async fn get_head_from_database(db: &Rexie) -> Result<ExtendedHeader> {
    let tx = db.transaction(
        &[HEADER_STORE_NAME, RANGES_STORE_NAME],
        TransactionMode::ReadOnly,
    )?;
    let header_store = tx.store(HEADER_STORE_NAME)?;
    let ranges_store = tx.store(RANGES_STORE_NAME)?;

    let ranges = get_ranges(&ranges_store, HEADER_RANGES_KEY).await?;
    let head_height = ranges.head().ok_or(StoreError::NotFound)?;

    get_by_height(&header_store, head_height).await
}

async fn get_by_height(header_store: &rexie::Store, height: u64) -> Result<ExtendedHeader> {
    let height_index = header_store.index(HEIGHT_INDEX_NAME)?;

    let height_key = to_value(&height)?;
    let header_entry = height_index.get(&height_key).await?;

    // querying unset key returns empty value
    if header_entry.is_falsy() {
        return Err(StoreError::NotFound);
    }

    let serialized_header = from_value::<ExtendedHeaderEntry>(header_entry)?.header;
    ExtendedHeader::decode(serialized_header.as_ref())
        .map_err(|e| StoreError::CelestiaTypes(e.into()))
}

async fn verify_against_neighbours(
    header_store: &rexie::Store,
    lowest_header: Option<&ExtendedHeader>,
    highest_header: Option<&ExtendedHeader>,
) -> Result<()> {
    if let Some(lowest_header) = lowest_header {
        let prev = get_by_height(header_store, lowest_header.height().value() - 1)
            .await
            .map_err(|e| {
                if let StoreError::NotFound = e {
                    StoreError::StoredDataError(
                        "inconsistency between headers and ranges table".into(),
                    )
                } else {
                    e
                }
            })?;
        prev.verify(lowest_header)?;
    }

    if let Some(highest_header) = highest_header {
        let next = get_by_height(header_store, highest_header.height().value() + 1)
            .await
            .map_err(|e| {
                if let StoreError::NotFound = e {
                    StoreError::StoredDataError(
                        "inconsistency between headers and ranges table".into(),
                    )
                } else {
                    e
                }
            })?;
        highest_header.verify(&next)?;
    }
    Ok(())
}

/// Similar to `get_schema_version` but detects older schema versions.
async fn get_schema_version_from_db(db: &Rexie) -> Result<Option<u32>> {
    let tx = db.transaction(
        &[HEADER_STORE_NAME, RANGES_STORE_NAME, SCHEMA_STORE_NAME],
        TransactionMode::ReadOnly,
    )?;
    let schema_store = tx.store(SCHEMA_STORE_NAME)?;
    let ranges_store = tx.store(RANGES_STORE_NAME)?;
    let header_store = tx.store(HEADER_STORE_NAME)?;

    // Schema version exists from v4 and above.
    if let Ok(version) = get_schema_version(&schema_store).await {
        return Ok(Some(version));
    }

    // If schema version does not exist in db but ranges store
    // has values then assume we are in version 3.
    if !store_is_empty(&ranges_store).await? {
        return Ok(Some(3));
    }

    // If ranges store does not have any values but header for height 1
    // exists, then we assume we are in version 2.
    let height_key = to_value(&1)?;
    let height_index = header_store.index(HEIGHT_INDEX_NAME)?;
    if height_index.get(&height_key).await?.is_truthy() {
        return Ok(Some(2));
    }

    // Otherwise we assume we have a new db.
    Ok(None)
}

async fn store_is_empty(store: &rexie::Store) -> Result<bool> {
    let vals = store.get_all(None, Some(1), None, None).await?;
    Ok(vals.is_empty())
}

async fn get_schema_version(store: &rexie::Store) -> Result<u32> {
    let key = to_value(VERSION_KEY)?;
    let val = store.get(&key).await?;
    Ok(from_value(val)?)
}

async fn set_schema_version(store: &rexie::Store, version: u32) -> Result<()> {
    let key = to_value(VERSION_KEY)?;
    let val = to_value(&version)?;
    store.put(&val, Some(&key)).await?;
    Ok(())
}

async fn get_last_header_v2(store: &rexie::Store) -> Result<ExtendedHeader> {
    let store_head = store
        .get_all(None, Some(1), None, Some(Direction::Prev))
        .await?
        .first()
        .ok_or(StoreError::NotFound)?
        .1
        .to_owned();

    let serialized_header = from_value::<ExtendedHeaderEntry>(store_head)?.header;

    ExtendedHeader::decode(serialized_header.as_ref())
        .map_err(|e| StoreError::CelestiaTypes(e.into()))
}

async fn migrate_older_to_v4(db: &Rexie) -> Result<()> {
    let Some(version) = get_schema_version_from_db(db).await? else {
        // New database.
        return Ok(());
    };

    if version >= 4 {
        // Nothing to migrate.
        return Ok(());
    }

    warn!("Migrating DB schema from v{version} to v4");

    let tx = db.transaction(
        &[HEADER_STORE_NAME, RANGES_STORE_NAME, SCHEMA_STORE_NAME],
        TransactionMode::ReadWrite,
    )?;
    let header_store = tx.store(HEADER_STORE_NAME)?;
    let ranges_store = tx.store(RANGES_STORE_NAME)?;
    let schema_store = tx.store(SCHEMA_STORE_NAME)?;

    let mut ranges = BlockRanges::default();

    if version <= 2 {
        match get_last_header_v2(&header_store).await {
            // On v2 there were no gaps between headers.
            Ok(head) => ranges.insert_relaxed(1..=head.height().value())?,
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e),
        }
    } else {
        // On v3 ranges existed but in different format.
        for (_, raw_range) in ranges_store
            .get_all(None, None, None, Some(Direction::Next))
            .await?
        {
            let (start, end) = from_value::<(u64, u64)>(raw_range)?;
            ranges.insert_relaxed(start..=end)?;
        }
    }

    ranges_store.clear().await?;
    set_ranges(&ranges_store, HEADER_RANGES_KEY, &ranges).await?;

    // Migrated to version 4
    set_schema_version(&schema_store, 4).await?;

    tx.commit().await?;

    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::store::utils::ExtendedHeaderGeneratorExt;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use function_name::named;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[named]
    #[wasm_bindgen_test]
    async fn test_large_db() {
        let store_name = function_name!();
        Rexie::delete(store_name).await.unwrap();
        let s = IndexedDbStore::new(store_name)
            .await
            .expect("creating test store failed");

        let mut gen = ExtendedHeaderGenerator::new();
        let expected_height = 1_000;

        s.insert(gen.next_many_verified(expected_height))
            .await
            .expect("inserting test data failed");

        for h in 1..=expected_height {
            s.update_sampling_metadata(h, SamplingStatus::Accepted, vec![])
                .await
                .expect("marking sampled failed");
        }

        assert_eq!(s.get_head().unwrap().height().value(), expected_height);

        drop(s);
        // re-open the store, to force re-calculation of the cached heights
        let s = IndexedDbStore::new(store_name)
            .await
            .expect("re-opening large test store failed");

        assert_eq!(s.get_head().unwrap().height().value(), expected_height);
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_persistence() {
        let (original_store, mut gen) = gen_filled_store(0, function_name!()).await;
        let original_headers = gen.next_many_verified(20);

        original_store
            .insert(original_headers.clone())
            .await
            .expect("inserting test data failed");
        drop(original_store);

        let reopened_store = IndexedDbStore::new(function_name!())
            .await
            .expect("failed to reopen store");

        assert_eq!(
            original_headers.as_ref().last().unwrap().height().value(),
            reopened_store.head_height().await.unwrap()
        );
        for original_header in original_headers.as_ref() {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .await
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }

        let new_headers = gen.next_many_verified(10);
        reopened_store
            .insert(new_headers.clone())
            .await
            .expect("failed to insert data");
        drop(reopened_store);

        let final_headers = original_headers
            .into_iter()
            .chain(new_headers.into_iter())
            .collect::<Vec<_>>();

        let reopened_store = IndexedDbStore::new(function_name!())
            .await
            .expect("failed to reopen store");

        assert_eq!(
            final_headers.last().unwrap().height().value(),
            reopened_store.head_height().await.unwrap()
        );
        for original_header in &final_headers {
            let stored_header = reopened_store
                .get_by_height(original_header.height().value())
                .await
                .unwrap();
            assert_eq!(original_header, &stored_header);
        }
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_delete_db() {
        let (original_store, _) = gen_filled_store(3, function_name!()).await;
        assert_eq!(original_store.get_head_height().unwrap(), 3);

        original_store.delete_db().await.unwrap();

        let same_name_store = IndexedDbStore::new(function_name!())
            .await
            .expect("creating test store failed");

        assert!(matches!(
            same_name_store.get_head_height(),
            Err(StoreError::NotFound)
        ));
    }

    mod migration_v1 {
        use super::*;

        const PREVIOUS_DB_VERSION: u32 = 1;

        // this fn sets upt the store manually with v1 schema (original, ExtendedHeader only),
        // and fills it with `hs` headers. It is a caller responsibility to make sure provided
        // headers are correct and in order.
        async fn init_store(name: &str, hs: Vec<ExtendedHeader>) {
            Rexie::delete(name).await.unwrap();
            let rexie = Rexie::builder(name)
                .version(PREVIOUS_DB_VERSION)
                .add_object_store(
                    ObjectStore::new("headers")
                        .key_path("id")
                        .auto_increment(true)
                        .add_index(Index::new("hash", "hash").unique(true))
                        .add_index(Index::new("height", "height").unique(true)),
                )
                .build()
                .await
                .unwrap();

            let tx = rexie
                .transaction(&["headers"], TransactionMode::ReadWrite)
                .unwrap();
            let header_store = tx.store("headers").unwrap();

            for header in hs {
                let header_entry = ExtendedHeaderEntry {
                    height: header.height().value(),
                    hash: header.hash(),
                    header: header.encode_vec().unwrap(),
                };

                header_store
                    .add(&to_value(&header_entry).unwrap(), None)
                    .await
                    .unwrap();
            }
        }

        #[named]
        #[wasm_bindgen_test]
        async fn migration_test() {
            let store_name = function_name!();
            let mut gen = ExtendedHeaderGenerator::new();
            let headers = gen.next_many(20);

            init_store(store_name, headers.clone()).await;

            let store = IndexedDbStore::new(store_name)
                .await
                .expect("opening migrated store failed");

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
                .update_sampling_metadata(1, SamplingStatus::Accepted, vec![])
                .await
                .unwrap();
            let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
            assert_eq!(sampling_data.status, SamplingStatus::Accepted);
        }
    }

    // open IndexedDB with unique per-test name to avoid interference and make cleanup easier
    pub async fn gen_filled_store(
        amount: u64,
        name: &str,
    ) -> (IndexedDbStore, ExtendedHeaderGenerator) {
        Rexie::delete(name).await.unwrap();
        let s = IndexedDbStore::new(name)
            .await
            .expect("creating test store failed");
        let mut gen = ExtendedHeaderGenerator::new();

        s.insert(gen.next_many_verified(amount))
            .await
            .expect("inserting test data failed");

        (s, gen)
    }
}
