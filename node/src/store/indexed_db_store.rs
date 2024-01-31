use std::cell::RefCell;
use std::convert::Infallible;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use js_sys::Array;
use rexie::{Direction, Index, KeyRange, ObjectStore, Rexie, TransactionMode};
use send_wrapper::SendWrapper;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::JsValue;

use crate::store::{ExtendedHeaderMetadata, Result, Store, StoreError};

/// indexeddb version, needs to be incremented on every schema schange
const DB_VERSION: u32 = 4;

const HEADER_STORE_NAME: &str = "headers";
const METADATA_STORE_NAME: &str = "meatadata";

const HASH_INDEX_NAME: &str = "hash";
const HEIGHT_INDEX_NAME: &str = "height";
const ACCEPTANCE_INDEX_NAME: &str = "accepted";

#[derive(Serialize_repr, Deserialize_repr, Debug, PartialEq)]
#[repr(u8)]
enum HeaderAccepted {
    Rejected = 1,
    Accepted = 2,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExtendedHeaderEntry {
    id: u64,
    // We use those fields as indexes, names need to match ones in `add_index`
    height: u64,
    hash: Hash,
    header: Vec<u8>,
    accepted: Option<HeaderAccepted>,
}

/// A [`Store`] implementation based on a `IndexedDB` browser database.
#[derive(Debug)]
pub struct IndexedDbStore {
    // SendWrapper usage is safe in wasm because we're running on a single thread
    head: SendWrapper<RefCell<Option<ExtendedHeader>>>,
    highest_sampled_height: SendWrapper<RefCell<u64>>,
    db: SendWrapper<Rexie>,
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
            .add_object_store(ObjectStore::new(METADATA_STORE_NAME).key_path("height"))
            .build()
            .await
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        let db_head = match get_head_from_database(&rexie).await {
            Ok(v) => Some(v),
            Err(StoreError::NotFound) => None,
            Err(e) => return Err(e),
        };

        let last_sampled = get_highest_sampled_height_from_database(&rexie).await?;

        Ok(Self {
            head: SendWrapper::new(RefCell::new(db_head)),
            highest_sampled_height: SendWrapper::new(RefCell::new(last_sampled)),
            db: SendWrapper::new(rexie),
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
            id: header_height,
            height: header_height,
            hash: header.hash(),
            header: serialized_header.unwrap(),
            accepted: None,
        };

        let jsvalue_header = to_value(&header_entry)?;

        header_store.add(&jsvalue_header, None).await?;
        tx.commit().await?;

        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        self.head.replace(Some(header));

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

        height <= head_height
    }

    async fn mark_header_sampled(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        // quick check with contains_height, which uses cached head
        if !self.contains_height(height) {
            return Err(StoreError::NotFound);
        }

        let tx = self
            .db
            .transaction(&[METADATA_STORE_NAME], TransactionMode::ReadWrite)?;
        let metadata_store = tx.store(METADATA_STORE_NAME)?;

        //let height_index = header_store.index(HEIGHT_INDEX_NAME)?;
        let metadata = ExtendedHeaderMetadata {
            accepted,
            cids_sampled: cids,
        };
        let metadata_jsvalue = to_value(&metadata)?;

        let height_key = to_value(&height)?;
        metadata_store
            .put(&metadata_jsvalue, Some(&height_key))
            .await?;
        //let height_entry_jsvalue = metadata_store.get(&height_key).await?;

        /*

        if height_entry_jsvalue.is_falsy() {
            return Err(StoreError::LostHeight(height));
        }

        let mut header_entry = from_value::<ExtendedHeaderEntry>(height_entry_jsvalue)?;
        header_entry.accepted = Some(accepted.into());

        let jsvalue_header = to_value(&header_entry)?;
        header_store.put(&jsvalue_header, None).await?;
        */

        tx.commit().await?;

        let new_highest_sampled_height = get_highest_sampled_height_from_database(&self.db).await?;
        self.highest_sampled_height
            .replace(new_highest_sampled_height);

        Ok(new_highest_sampled_height)
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

    async fn highest_sampled_height(&self) -> Result<u64> {
        // this shouldn't panic, we don't borrow across await points and wasm is single threaded
        Ok(*self.highest_sampled_height.borrow())
    }

    async fn mark_header_sampled(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64> {
        let fut = SendWrapper::new(self.mark_header_sampled(height, accepted, cids));
        fut.await
    }
}

impl From<rexie::Error> for StoreError {
    fn from(error: rexie::Error) -> StoreError {
        use rexie::Error as E;
        match error {
            e @ E::AsyncChannelError => StoreError::ExecutorError(e.to_string()),
            other => StoreError::BackingStoreError(other.to_string()),
        }
    }
}

impl From<serde_wasm_bindgen::Error> for StoreError {
    fn from(error: serde_wasm_bindgen::Error) -> StoreError {
        StoreError::StoredDataError(format!("Error de/serializing: {error}"))
    }
}

impl From<HeaderAccepted> for bool {
    fn from(accepted: HeaderAccepted) -> bool {
        accepted == HeaderAccepted::Accepted
    }
}

impl From<bool> for HeaderAccepted {
    fn from(accepted: bool) -> Self {
        if accepted {
            HeaderAccepted::Accepted
        } else {
            HeaderAccepted::Rejected
        }
    }
}

async fn get_highest_sampled_height_from_database(db: &Rexie) -> Result<u64> {
    let last_accepted = KeyRange::only(&JsValue::from(HeaderAccepted::Accepted as u8))?;
    let last_rejected = KeyRange::only(&JsValue::from(HeaderAccepted::Rejected as u8))?;
    /*
    let sampled_key = &KeyRange::bound(
            &Array::of2(
                &JsValue::from(1),
                &JsValue::from(HeaderAccepted::Rejected as u8),
            ),
            &Array::of2(
                &JsValue::from(u64::MAX),
                &JsValue::from(HeaderAccepted::Accepted as u8),
            ),
            false,
            false,
            ).unwrap();
    */
    /*
    let sampled_key = KeyRange::bound(
        &JsValue::from(HeaderAccepted::Rejected as u8),
        &JsValue::from(HeaderAccepted::Accepted as u8 + 1),
        false,
        false,
    )
    .unwrap();
    */

    let tx = db.transaction(&[HEADER_STORE_NAME], TransactionMode::ReadOnly)?;
    let store = tx.store(HEADER_STORE_NAME)?;
    let acceptance_index = store.index(ACCEPTANCE_INDEX_NAME)?;

    let last_accepted = acceptance_index
        .get_all(Some(&last_accepted), Some(1), None, Some(Direction::Prev))
        .await?
        .pop();
    let last_rejected = acceptance_index
        .get_all(Some(&last_rejected), Some(1), None, Some(Direction::Prev))
        .await?
        .pop();

    /*
    let Some(last_sampled) = acceptance_index
        .get_all(Some(&sampled_key), Some(1), None, Some(Direction::Prev))
        .await?
        .pop()
    else {
        return Ok(0);
    };
    let last_sampled = u64::max(last_accepted.unwrap_or(0), last_rejected.unwrap_or(0));
    */

    //println!("highest: {:?}", last_sampled);

    //Ok(from_value::<ExtendedHeaderEntry>(last_sampled.1)?.height)
    todo!()
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;
    use function_name::named;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[named]
    #[wasm_bindgen_test]
    async fn test_empty_store() {
        let s = gen_filled_store(0, function_name!()).await.0;
        assert!(matches!(s.get_head_height(), Err(StoreError::NotFound)));
        assert!(matches!(s.get_head(), Err(StoreError::NotFound)));
        assert!(matches!(
            s.get_by_height(1).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            s.get_by_hash(&Hash::Sha256([0; 32])).await,
            Err(StoreError::NotFound)
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_read_write() {
        let (s, mut gen) = gen_filled_store(0, function_name!()).await;

        let header = gen.next();

        s.append_single_unchecked(header.clone()).await.unwrap();
        assert_eq!(s.get_head_height().unwrap(), 1);
        assert_eq!(s.get_head().unwrap(), header);
        assert_eq!(s.get_by_height(1).await.unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_pregenerated_data() {
        let (s, _) = gen_filled_store(100, function_name!()).await;
        assert_eq!(s.get_head_height().unwrap(), 100);
        let head = s.get_head().unwrap();
        assert_eq!(s.get_by_height(100).await.unwrap(), head);
        assert!(matches!(
            s.get_by_height(101).await,
            Err(StoreError::NotFound)
        ));

        let header = s.get_by_height(54).await.unwrap();
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_duplicate_insert() {
        let (s, mut gen) = gen_filled_store(100, function_name!()).await;
        let header101 = gen.next();
        s.append_single_unchecked(header101.clone()).await.unwrap();
        //assert_eq!(s.append_single_unchecked(header101.clone()).await.unwrap(), ());
        assert!(matches!(
            s.append_single_unchecked(header101).await,
            Err(StoreError::HeightExists(101))
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_overwrite_height() {
        let (s, gen) = gen_filled_store(100, function_name!()).await;

        // Height 30 with different hash
        let header29 = s.get_by_height(29).await.unwrap();
        let header30 = gen.next_of(&header29);

        let insert_existing_result = s.append_single_unchecked(header30).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HeightExists(30))
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_overwrite_hash() {
        let (s, _) = gen_filled_store(100, function_name!()).await;
        let mut dup_header = s.get_by_height(33).await.unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_single_unchecked(dup_header).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::HashExists(_))
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_append_range() {
        let (s, mut gen) = gen_filled_store(10, function_name!()).await;
        let hs = gen.next_many(4);
        s.append_unchecked(hs).await.unwrap();
        s.get_by_height(14).await.unwrap();
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_append_gap_between_head() {
        let (s, mut gen) = gen_filled_store(10, function_name!()).await;

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

    #[named]
    #[wasm_bindgen_test]
    async fn test_non_continuous_append() {
        let (s, mut gen) = gen_filled_store(10, function_name!()).await;
        let mut hs = gen.next_many(6);

        // remove height 14
        hs.remove(3);

        let insert_existing_result = s.append_unchecked(hs).await;
        assert!(matches!(
            insert_existing_result,
            Err(StoreError::NonContinuousAppend(13, 15))
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_genesis_with_height() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        let s = gen_filled_store(0, function_name!()).await.0;

        assert!(matches!(
            s.append_single_unchecked(header5).await,
            Err(StoreError::NonContinuousAppend(0, 5))
        ));
    }

    #[named]
    #[wasm_bindgen_test]
    async fn test_large_db() {
        let s = IndexedDbStore::new(function_name!())
            .await
            .expect("creating test store failed");

        let next_height = s.get_head_height().unwrap_or(0) + 1;

        let mut gen = ExtendedHeaderGenerator::new_from_height(next_height);

        for _ in 0..=1_000 {
            s.append_single_unchecked(gen.next())
                .await
                .expect("inserting test data failed");
        }

        let expected_height = next_height + 1_000;
        assert_eq!(s.get_head().unwrap().height().value(), expected_height);
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

    #[named]
    #[wasm_bindgen_test]
    async fn test_sampling_height() {
        let (store, _) = gen_filled_store(9, function_name!()).await;

        store.mark_header_sampled(1, true, vec![]).await.unwrap();
        store.mark_header_sampled(2, true, vec![]).await.unwrap();
        store.mark_header_sampled(3, false, vec![]).await.unwrap();
        store.mark_header_sampled(4, true, vec![]).await.unwrap();
        store.mark_header_sampled(5, false, vec![]).await.unwrap();
        store.mark_header_sampled(6, false, vec![]).await.unwrap();

        assert_eq!(store.highest_sampled_height().await.unwrap(), 6);
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
