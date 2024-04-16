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

use crate::store::{Result, SamplingMetadata, Store, StoreError};

/// indexeddb version, needs to be incremented on every schema schange
const DB_VERSION: u32 = 2;

// Data stores (SQL table analogue) used in IndexedDb
const HEADER_STORE_NAME: &str = "headers";
const SAMPLING_STORE_NAME: &str = "sampling";

// Additional indexes set on HEADER_STORE, for querying by height and hash
const HASH_INDEX_NAME: &str = "hash";
const HEIGHT_INDEX_NAME: &str = "height";

// Maximumum number of headers to fetch at once when updating sampling height from database
const MAX_UNSAMPLED_HEIGHT_SCAN_BATCH_SIZE: u32 = 100;

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
    lowest_unsampled_height: SendWrapper<RefCell<u64>>,
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
            .add_object_store(ObjectStore::new(SAMPLING_STORE_NAME))
            .build()
            .await
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        let db_head = match get_head_from_database(&rexie).await {
            Ok(v) => Some(v),
            Err(StoreError::NotFound) => None,
            Err(e) => return Err(e),
        };

        // 1 is lowest valid header heigth, start from there
        let last_sampled = get_next_unsampled_height_from_database(&rexie, 1).await?;

        Ok(Self {
            head: SendWrapper::new(RefCell::new(db_head)),
            lowest_unsampled_height: SendWrapper::new(RefCell::new(last_sampled)),
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
        // quick check with contains_height, which uses cached head
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let tx = self
            .db
            .transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;
        let header_store = tx.store(HEADER_STORE_NAME)?;
        let height_index = header_store.index(HEIGHT_INDEX_NAME)?;

        let height_key = to_value(&height)?;
        let header_entry = height_index.get(&height_key).await?;

        // querying unset key returns empty value
        if header_entry.is_falsy() {
            return Err(StoreError::LostHeight(height));
        }

        let serialized_header = from_value::<ExtendedHeaderEntry>(header_entry)?.header;
        ExtendedHeader::decode(serialized_header.as_ref())
            .map_err(|e| StoreError::CelestiaTypes(e.into()))
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

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        let height = header.height().value();
        let hash = header.hash();

        let head_height = self.get_head_height().unwrap_or(0);

        // A light check before checking the whole map
        if head_height > 0 && height <= head_height {
            return Err(StoreError::HeightExists(height));
        }

        // Check if it's continuous before checking the whole map.
        if head_height + 1 != height {
            return Err(StoreError::NonContinuousAppend(head_height, height));
        }

        let tx = self
            .db
            .transaction(&[HEADER_STORE_NAME], TransactionMode::ReadWrite)?;
        let header_store = tx.store(HEADER_STORE_NAME)?;

        let height_index = header_store.index(HEIGHT_INDEX_NAME)?;
        let jsvalue_height_key = KeyRange::only(&to_value(&height)?)?;
        if height_index
            .count(Some(&jsvalue_height_key))
            .await
            .unwrap_or(0)
            != 0
        {
            return Err(StoreError::HeightExists(height));
        }

        let hash_index = header_store.index(HASH_INDEX_NAME)?;
        let jsvalue_hash_key = KeyRange::only(&to_value(&hash)?)?;
        if hash_index.count(Some(&jsvalue_hash_key)).await.unwrap_or(0) != 0 {
            return Err(StoreError::HashExists(hash));
        }

        // make sure Result is Infallible, we unwrap it later
        let serialized_header: std::result::Result<_, Infallible> = header.encode_vec();

        let header_height = header.height().value();
        let header_entry = ExtendedHeaderEntry {
            height: header_height,
            hash: header.hash(),
            header: serialized_header.unwrap(),
        };

        let jsvalue_header = to_value(&header_entry)?;

        header_store.add(&jsvalue_header, None).await?;
        tx.commit().await?;

        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        self.head.replace(Some(header));
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

    fn contains_height(&self, height: u64) -> bool {
        let Ok(head_height) = self.get_head_height() else {
            return false;
        };

        height > 0 && height <= head_height
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        // quick check with contains_height, which uses cached head
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let height_key = to_value(&height)?;

        let tx = self
            .db
            .transaction(&[SAMPLING_STORE_NAME], TransactionMode::ReadWrite)?;
        let sampling_store = tx.store(SAMPLING_STORE_NAME)?;

        let previous_entry = sampling_store.get(&height_key).await?;
        let new_entry = if previous_entry.is_falsy() {
            SamplingMetadata {
                accepted,
                cids_sampled: cids,
            }
        } else {
            let mut value: SamplingMetadata = from_value(previous_entry)?;
            value.accepted = accepted;

            for cid in &cids {
                if !value.cids_sampled.contains(cid) {
                    value.cids_sampled.push(cid.to_owned());
                }
            }

            value
        };

        let metadata_jsvalue = to_value(&new_entry)?;

        sampling_store
            .put(&metadata_jsvalue, Some(&height_key))
            .await?;

        tx.commit().await?;

        self.update_unsampled_height().await
    }

    async fn update_unsampled_height(&self) -> Result<u64> {
        loop {
            let previously_lowest_unsampled = *self.lowest_unsampled_height.borrow();
            let new_highest_sampled_height =
                get_next_unsampled_height_from_database(&self.db, previously_lowest_unsampled)
                    .await?;

            // wasm is single threaded, but the value could have changed on await point, re-check
            if previously_lowest_unsampled != *self.lowest_unsampled_height.borrow() {
                continue;
            }

            self.lowest_unsampled_height
                .replace(new_highest_sampled_height);
            return Ok(new_highest_sampled_height);
        }
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        if !self.contains_height(height) {
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

    async fn wait_height(&self, height: u64) -> Result<()> {
        let mut notifier = pin!(self.header_added_notifier.notified());

        loop {
            if self.contains_height(height) {
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
        self.contains_height(height)
    }

    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
        let fut = SendWrapper::new(self.append_single_unchecked(header));
        fut.await
    }

    async fn next_unsampled_height(&self) -> Result<u64> {
        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        Ok(*self.lowest_unsampled_height.borrow())
    }

    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        let fut = SendWrapper::new(self.update_sampling_metadata(height, accepted, cids));
        fut.await
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        let fut = SendWrapper::new(self.get_sampling_metadata(height));
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

async fn get_head_from_database(db: &Rexie) -> Result<ExtendedHeader> {
    let tx = db.transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;
    let store = tx.store(HEADER_STORE_NAME)?;

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

/// Return the height of the lowest unsampled header from the database, starting
/// from `last_known_unsampled` offset
async fn get_next_unsampled_height_from_database(
    db: &Rexie,
    last_known_unsampled: u64,
) -> Result<u64> {
    let mut starting_index = last_known_unsampled;
    let tx = db.transaction(&[SAMPLING_STORE_NAME], TransactionMode::ReadOnly)?;
    let store = tx.store(SAMPLING_STORE_NAME)?;

    loop {
        let pending_headers_keyrange = KeyRange::lower_bound(&to_value(&starting_index)?, false)?;
        let pending_sampled_headers = store
            .get_all(
                Some(&pending_headers_keyrange),
                Some(MAX_UNSAMPLED_HEIGHT_SCAN_BATCH_SIZE),
                None,
                Some(Direction::Next),
            )
            .await?;

        let mut potential_new_height = starting_index;
        for (height, _) in pending_sampled_headers {
            if height.as_f64() == Some(potential_new_height as f64) {
                potential_new_height += 1;
            } else {
                break;
            }
        }

        if potential_new_height - starting_index == MAX_UNSAMPLED_HEIGHT_SCAN_BATCH_SIZE as u64 {
            starting_index += MAX_UNSAMPLED_HEIGHT_SCAN_BATCH_SIZE as u64;
        } else {
            return Ok(potential_new_height);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
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

        for h in 1..=expected_height {
            s.append_single_unchecked(gen.next())
                .await
                .expect("inserting test data failed");
            s.update_sampling_metadata(h, true, vec![])
                .await
                .expect("marking sampled failed");
        }

        assert_eq!(s.get_head().unwrap().height().value(), expected_height);
        assert_eq!(
            s.next_unsampled_height().await.unwrap(),
            expected_height + 1
        );

        drop(s);
        // re-open the store, to force re-calculation of the cached heights
        let s = IndexedDbStore::new(store_name)
            .await
            .expect("re-opening large test store failed");

        assert_eq!(s.get_head().unwrap().height().value(), expected_height);
        assert_eq!(
            s.next_unsampled_height().await.unwrap(),
            expected_height + 1
        );
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_persistence() {
        let (original_store, mut gen) = gen_filled_store(0, function_name!()).await;
        let mut original_headers = gen.next_many(20);

        for h in &original_headers {
            original_store
                .append_single_unchecked(h.clone())
                .await
                .expect("inserting test data failed");
        }
        drop(original_store);

        let reopened_store = IndexedDbStore::new(function_name!())
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

        let reopened_store = IndexedDbStore::new(function_name!())
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
                .update_sampling_metadata(1, true, vec![])
                .await
                .unwrap();
            let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
            assert!(sampling_data.accepted);
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

        let headers = gen.next_many(amount);

        for header in headers {
            s.append_single_unchecked(header)
                .await
                .expect("inserting test data failed");
        }

        (s, gen)
    }
}
