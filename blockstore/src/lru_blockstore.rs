use std::{num::NonZeroUsize, sync::Mutex};

use cid::CidGeneric;
use lru::LruCache;

use crate::{convert_cid, Blockstore, Result};

/// An LRU cached [`Blockstore`].
pub struct LruBlockstore<const MAX_MULTIHASH_SIZE: usize> {
    cache: Mutex<LruCache<CidGeneric<MAX_MULTIHASH_SIZE>, Vec<u8>>>,
}

impl<const MAX_MULTIHASH_SIZE: usize> LruBlockstore<MAX_MULTIHASH_SIZE> {
    /// Creates a new LRU cached [`Blockstore`] that holds at most `capacity` items.
    pub fn new(capacity: NonZeroUsize) -> Self {
        LruBlockstore {
            cache: Mutex::new(LruCache::new(capacity)),
        }
    }
}

#[cfg_attr(not(docs_rs), async_trait::async_trait)]
impl<const MAX_MULTIHASH_SIZE: usize> Blockstore for LruBlockstore<MAX_MULTIHASH_SIZE> {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        let cid = convert_cid(cid)?;
        let mut cache = self.cache.lock().expect("lock failed");
        Ok(cache.get(&cid).map(ToOwned::to_owned))
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid = convert_cid(cid)?;
        let mut cache = self.cache.lock().expect("lock failed");
        if cache.contains(&cid) {
            cache.promote(&cid);
        } else {
            cache.put(cid, data.to_vec());
        }
        Ok(())
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let cid = convert_cid(cid)?;
        let cache = self.cache.lock().expect("lock failed");
        Ok(cache.contains(&cid))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    use crate::tests::cid_v1;

    use super::*;

    #[async_test]
    async fn insert_get_overflowing_cache_size() {
        // Blockstore that can hold the last 2 items.
        let store = LruBlockstore::<64>::new(NonZeroUsize::new(2).unwrap());

        let cid1 = cid_v1::<64>(b"1");
        let cid2 = cid_v1::<64>(b"2");
        let cid3 = cid_v1::<64>(b"3");

        store.put_keyed(&cid1, b"1").await.unwrap();
        assert_eq!(store.get(&cid1).await.unwrap().unwrap(), b"1");

        store.put_keyed(&cid2, b"2").await.unwrap();
        assert_eq!(store.get(&cid2).await.unwrap().unwrap(), b"2");
        assert!(store.has(&cid1).await.unwrap());

        store.put_keyed(&cid3, b"3").await.unwrap();
        assert_eq!(store.get(&cid3).await.unwrap().unwrap(), b"3");
        assert!(store.has(&cid2).await.unwrap());
        assert!(!store.has(&cid1).await.unwrap());
    }
}
