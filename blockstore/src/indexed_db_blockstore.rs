use cid::CidGeneric;
use js_sys::Uint8Array;
use rexie::{KeyRange, ObjectStore, Rexie, Store, TransactionMode};
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast, JsValue};

use crate::{convert_cid, Blockstore, BlockstoreError, Result};

/// indexeddb version, needs to be incremented on every schema schange
const DB_VERSION: u32 = 1;

const BLOCKS_STORE: &str = "BLOCKSTORE.BLOCKS";

/// A [`Blockstore`] implementation backed by a [`sled`] database.
#[derive(Debug)]
pub struct IndexedDbBlockstore<const MAX_MULTIHASH_SIZE: usize> {
    db: SendWrapper<Rexie>,
}

impl<const MAX_MULTIHASH_SIZE: usize> IndexedDbBlockstore<MAX_MULTIHASH_SIZE> {
    /// Create or open a [`IndexedDbBlockstore`] in a given sled [`Db`].
    ///
    /// # Example
    /// ```
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// use blockstore::IndexedDbBlockstore;
    /// use tokio::task::spawn_blocking;
    ///
    /// let db = spawn_blocking(|| sled::open("path/to/db")).await??;
    /// let blockstore = IndexedDbBlockstore::<64>::new(db).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(name: &str) -> Result<Self> {
        let rexie = Rexie::builder(name)
            .version(DB_VERSION)
            .add_object_store(
                ObjectStore::new(BLOCKS_STORE)
                    .auto_increment(false),
            )
            .build()
            .await
            .map_err(|e| BlockstoreError::BackingStoreError(e.to_string()))?;

        Ok(Self {
            db: SendWrapper::new(rexie),
        })
    }

    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let cid = Uint8Array::from(cid.to_bytes().as_ref());

        let tx = self
            .db
            .transaction(&[BLOCKS_STORE], TransactionMode::ReadOnly)?;
        let blocks = tx.store(BLOCKS_STORE)?;
        let block = blocks.get(&cid).await?;

        if block.is_undefined() {
            Ok(None)
        } else {
            let arr = block.dyn_ref::<Uint8Array>().ok_or_else(|| {
                BlockstoreError::StorageCorrupted(format!(
                    "expected 'Uint8Array', got '{}'",
                    block
                        .js_typeof()
                        .as_string()
                        .expect("typeof must be a string")
                ))
            })?;
            Ok(Some(arr.to_vec()))
        }
    }

    async fn put<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let cid = Uint8Array::from(cid.to_bytes().as_ref());
        let data = Uint8Array::from(data);

        let tx = self
            .db
            .transaction(&[BLOCKS_STORE], TransactionMode::ReadWrite)?;
        let blocks = tx.store(BLOCKS_STORE)?;

        if !has_key(&blocks, &cid).await? {
            blocks.add(&data, Some(&cid)).await?;
            Ok(())
        } else {
            Err(BlockstoreError::CidExists)
        }
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let cid = Uint8Array::from(cid.to_bytes().as_ref());

        let tx = self
            .db
            .transaction(&[BLOCKS_STORE], TransactionMode::ReadOnly)?;
        let blocks = tx.store(BLOCKS_STORE)?;

        has_key(&blocks, &cid).await
    }
}

#[cfg_attr(not(docs_rs), async_trait::async_trait)]
impl<const MAX_MULTIHASH_SIZE: usize> Blockstore for IndexedDbBlockstore<MAX_MULTIHASH_SIZE> {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        let fut = SendWrapper::new(self.get(cid));
        fut.await
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let fut = SendWrapper::new(self.put(cid, data));
        fut.await
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let fut = SendWrapper::new(self.has(cid));
        fut.await
    }
}

impl From<rexie::Error> for BlockstoreError {
    fn from(value: rexie::Error) -> Self {
        BlockstoreError::BackingStoreError(value.to_string())
    }
}

async fn has_key(store: &Store, key: &JsValue) -> Result<bool> {
    let key_range = KeyRange::only(key)?;
    let count = store.count(Some(&key_range)).await?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use crate::test_utils::{blockstore_tests, cid_v1};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);
    blockstore_tests!(create_unique_store, wasm_bindgen_test);

    #[wasm_bindgen_test]
    async fn store_persists() {
        let store_name = "indexeddb-blockstore-test-persistent";
        Rexie::delete(store_name).await.unwrap();

        let store = IndexedDbBlockstore::<64>::new(store_name).await.unwrap();
        let cid = cid_v1::<64>(b"1");
        let data = b"data";

        store.put_keyed(&cid, data).await.unwrap();

        store.db.take().close();

        let store = IndexedDbBlockstore::<64>::new(store_name).await.unwrap();
        let received = store.get(&cid).await.unwrap();

        assert_eq!(received, Some(data.to_vec()));
    }

    async fn create_store<const S: usize>(name: &str) -> IndexedDbBlockstore<S> {
        // the db's don't seem to persist but for extra safety make a cleanup
        Rexie::delete(name).await.unwrap();
        IndexedDbBlockstore::new(name).await.unwrap()
    }

    async fn create_unique_store<const S: usize>() -> IndexedDbBlockstore<S> {
        static NAME: AtomicU32 = AtomicU32::new(0);

        let name = NAME.fetch_add(1, Ordering::SeqCst);
        let name = format!("indexeddb-blockstore-test-{name}");

        create_store(&name).await
    }
}
