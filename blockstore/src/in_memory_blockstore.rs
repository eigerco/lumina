use cid::CidGeneric;
use dashmap::DashMap;

use crate::{convert_cid, Blockstore, Result};

/// Simple in-memory blockstore implementation.
pub struct InMemoryBlockstore<const MAX_MULTIHASH_SIZE: usize> {
    map: DashMap<CidGeneric<MAX_MULTIHASH_SIZE>, Vec<u8>>,
}

impl<const MAX_MULTIHASH_SIZE: usize> InMemoryBlockstore<MAX_MULTIHASH_SIZE> {
    /// Create new empty in-memory blockstore
    pub fn new() -> Self {
        InMemoryBlockstore {
            map: DashMap::new(),
        }
    }

    fn get_cid(&self, cid: &CidGeneric<MAX_MULTIHASH_SIZE>) -> Result<Option<Vec<u8>>> {
        Ok(self.map.get(cid).as_deref().cloned())
    }

    fn insert_cid(&self, cid: CidGeneric<MAX_MULTIHASH_SIZE>, data: &[u8]) -> Result<()> {
        self.map.entry(cid).or_insert_with(|| data.to_vec());

        Ok(())
    }

    fn contains_cid(&self, cid: &CidGeneric<MAX_MULTIHASH_SIZE>) -> bool {
        self.map.contains_key(cid)
    }
}

#[cfg_attr(not(docs_rs), async_trait::async_trait)]
impl<const MAX_MULTIHASH_SIZE: usize> Blockstore for InMemoryBlockstore<MAX_MULTIHASH_SIZE> {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        let cid = convert_cid(cid)?;
        self.get_cid(&cid)
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid = convert_cid(cid)?;
        self.insert_cid(cid, data)
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let cid = convert_cid(cid)?;
        Ok(self.contains_cid(&cid))
    }
}

impl<const MAX_MULTIHASH_SIZE: usize> Default for InMemoryBlockstore<MAX_MULTIHASH_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}
