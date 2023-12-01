use async_trait::async_trait;
use cid::CidGeneric;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use multihash::Multihash;

use crate::{Blockstore, BlockstoreError, Result};

pub struct InMemoryBlockstore<const S: usize> {
    map: DashMap<CidGeneric<S>, Vec<u8>>,
}

impl<const S: usize> InMemoryBlockstore<S> {
    pub fn new() -> Self {
        InMemoryBlockstore {
            map: DashMap::new(),
        }
    }

    fn get_cid(&self, cid: &CidGeneric<S>) -> Result<Vec<u8>> {
        self.map
            .get(cid)
            .as_deref()
            .cloned()
            .ok_or(BlockstoreError::CidNotFound)
    }

    fn insert_cid(&self, cid: CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid_entry = self.map.entry(cid);
        if matches!(cid_entry, Entry::Occupied(_)) {
            return Err(BlockstoreError::CidExists);
        }

        cid_entry.insert(data.to_vec());

        Ok(())
    }
}

#[async_trait]
impl<const S: usize> Blockstore for InMemoryBlockstore<S> {
    async fn get<const SS: usize>(&self, cid: &CidGeneric<SS>) -> Result<Vec<u8>> {
        let hash = cid.hash();
        let hash = Multihash::<S>::wrap(hash.code(), hash.digest())
            .map_err(|_| BlockstoreError::CidTooLong)?;
        let cid = CidGeneric::<S>::new_v1(cid.codec(), hash);

        self.get_cid(&cid)
    }

    async fn insert<const SS: usize>(&self, cid: &CidGeneric<SS>, data: &[u8]) -> Result<()> {
        let hash = cid.hash();
        let hash = Multihash::<S>::wrap(hash.code(), hash.digest())
            .map_err(|_| BlockstoreError::CidTooLong)?;
        let cid = CidGeneric::<S>::new_v1(cid.codec(), hash);

        self.insert_cid(cid, data)
    }
}

impl<const S: usize> Default for InMemoryBlockstore<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_get() {
        let store = InMemoryBlockstore::<128>::new();

        let cid = CidGeneric::<128>::read_bytes(
            [
                0x01, // CIDv1
                0x01, // CID codec = 1
                0x02, // code = 2
                0x03, // len = 3
                1, 2, 3, // hash
            ]
            .as_ref(),
        )
        .unwrap();
        let data = [0xCD; 512];

        store.insert(&cid, &data).await.unwrap();

        let retrieved_data = store.get(&cid).await.unwrap();
        assert_eq!(data, retrieved_data.as_ref());

        let another_cid = CidGeneric::<128>::default();
        let missing_block = store.get(&another_cid).await.unwrap_err();
        assert_eq!(missing_block, BlockstoreError::CidNotFound);
    }

    #[tokio::test]
    async fn test_duplicate_insert() {
        let cid0 = CidGeneric::<128>::read_bytes(
            [
                0x01, // CIDv1
                0x11, // CID codec
                0x22, // multihash code
                0x02, // len = 2
                0, 0, // hash
            ]
            .as_ref(),
        )
        .unwrap();
        let cid1 = CidGeneric::<128>::read_bytes(
            [
                0x01, // CIDv1
                0x33, // CID codec
                0x44, // multihash code
                0x02, // len = 2
                0, 1, // hash
            ]
            .as_ref(),
        )
        .unwrap();

        let store = InMemoryBlockstore::<128>::new();

        store.insert(&cid0, &[0x01; 1]).await.unwrap();
        store.insert(&cid1, &[0x02; 1]).await.unwrap();
        let insert_err = store.insert(&cid1, &[0x03; 1]).await.unwrap_err();
        assert_eq!(insert_err, BlockstoreError::CidExists);
    }

    #[tokio::test]
    async fn different_cid_size() {
        let cid_bytes = [
            0x01, // CIDv1
            0x11, // CID codec
            0x22, // multihash code
            0x02, // len = 2
            0, 0, // hash
        ];
        let cid0 = CidGeneric::<6>::read_bytes(cid_bytes.as_ref()).unwrap();
        let cid1 = CidGeneric::<32>::read_bytes(cid_bytes.as_ref()).unwrap();
        let cid2 = CidGeneric::<64>::read_bytes(cid_bytes.as_ref()).unwrap();
        let cid3 = CidGeneric::<65>::read_bytes(cid_bytes.as_ref()).unwrap();
        let cid4 = CidGeneric::<128>::read_bytes(cid_bytes.as_ref()).unwrap();

        let data = [0x99; 5];

        let store = InMemoryBlockstore::<64>::new();
        store.insert(&cid0, data.as_ref()).await.unwrap();

        let data0 = store.get(&cid0).await.unwrap();
        assert_eq!(data, data0.as_ref());
        let data1 = store.get(&cid1).await.unwrap();
        assert_eq!(data, data1.as_ref());
        let data2 = store.get(&cid2).await.unwrap();
        assert_eq!(data, data2.as_ref());
        let data3 = store.get(&cid3).await.unwrap();
        assert_eq!(data, data3.as_ref());
        let data4 = store.get(&cid4).await.unwrap();
        assert_eq!(data, data4.as_ref());
    }

    #[tokio::test]
    async fn too_large_cid() {
        let cid_bytes = [
            0x01, // CIDv1
            0x11, // CID codec
            0x22, // multihash code
            0x10, // len = 16
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let cid = CidGeneric::<32>::read_bytes(cid_bytes.as_ref()).unwrap();

        let store = InMemoryBlockstore::<8>::new();
        let insert_err = store.insert(&cid, [0x00, 1].as_ref()).await.unwrap_err();
        assert_eq!(insert_err, BlockstoreError::CidTooLong);

        let insert_err = store.get(&cid).await.unwrap_err();
        assert_eq!(insert_err, BlockstoreError::CidTooLong);
    }
}
