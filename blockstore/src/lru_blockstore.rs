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
        let mut cache = self.cache.lock().unwrap();
        Ok(cache.get(&cid).map(ToOwned::to_owned))
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid = convert_cid(cid)?;
        let mut cache = self.cache.lock().unwrap();
        cache.put(cid, data.to_vec());
        Ok(())
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let cid = convert_cid(cid)?;
        let cache = self.cache.lock().unwrap();
        Ok(cache.contains(&cid))
    }
}
