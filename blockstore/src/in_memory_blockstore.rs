use cid::CidGeneric;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

use crate::{convert_cid, Blockstore, BlockstoreError, Result};

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
        let cid_entry = self.map.entry(cid);
        if matches!(cid_entry, Entry::Occupied(_)) {
            return Err(BlockstoreError::CidExists);
        }

        cid_entry.insert(data.to_vec());

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{Block, CidError};
    use multihash::Multihash;
    use std::iter::zip;
    use std::result::Result as StdResult;

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

        store.put_keyed(&cid, &data).await.unwrap();

        let retrieved_data = store.get(&cid).await.unwrap().unwrap();
        assert_eq!(data, retrieved_data.as_ref());

        let another_cid = CidGeneric::<128>::default();
        let missing_block = store.get(&another_cid).await.unwrap();
        assert_eq!(missing_block, None);
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

        store.put_keyed(&cid0, &[0x01; 1]).await.unwrap();
        store.put_keyed(&cid1, &[0x02; 1]).await.unwrap();
        let insert_err = store.put_keyed(&cid1, &[0x03; 1]).await.unwrap_err();
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
        store.put_keyed(&cid0, data.as_ref()).await.unwrap();

        let data0 = store.get(&cid0).await.unwrap().unwrap();
        assert_eq!(data, data0.as_ref());
        let data1 = store.get(&cid1).await.unwrap().unwrap();
        assert_eq!(data, data1.as_ref());
        let data2 = store.get(&cid2).await.unwrap().unwrap();
        assert_eq!(data, data2.as_ref());
        let data3 = store.get(&cid3).await.unwrap().unwrap();
        assert_eq!(data, data3.as_ref());
        let data4 = store.get(&cid4).await.unwrap().unwrap();
        assert_eq!(data, data4.as_ref());
    }

    #[tokio::test]
    async fn too_large_cid() {
        let cid = CidGeneric::<32>::read_bytes(
            [
                0x01, // CIDv1
                0x11, // CID codec
                0x22, // multihash code
                0x10, // len = 16
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]
            .as_ref(),
        )
        .unwrap();

        let store = InMemoryBlockstore::<8>::new();
        let insert_err = store.put_keyed(&cid, [0x00, 1].as_ref()).await.unwrap_err();
        assert_eq!(insert_err, BlockstoreError::CidTooLong);

        let insert_err = store.get(&cid).await.unwrap_err();
        assert_eq!(insert_err, BlockstoreError::CidTooLong);
    }

    #[tokio::test]
    async fn test_block_insert() {
        let block = TestBlock([0, 1, 2, 3]);

        let store = InMemoryBlockstore::<8>::new();
        store.put(block).await.unwrap();
        let retrieved_block = store.get(&block.cid().unwrap()).await.unwrap().unwrap();
        assert_eq!(block.data(), &retrieved_block);
    }

    #[tokio::test]
    async fn test_multiple_blocks_insert() {
        let blocks = [
            TestBlock([0, 0, 0, 0]),
            TestBlock([0, 0, 0, 1]),
            TestBlock([0, 0, 1, 0]),
            TestBlock([0, 0, 1, 1]),
            TestBlock([0, 1, 0, 0]),
            TestBlock([0, 1, 0, 1]),
            TestBlock([0, 1, 1, 0]),
            TestBlock([0, 1, 1, 1]),
        ];
        let uninserted_blocks = [
            TestBlock([1, 0, 0, 0]),
            TestBlock([1, 0, 0, 1]),
            TestBlock([1, 0, 1, 0]),
            TestBlock([1, 1, 0, 1]),
        ];

        let store = InMemoryBlockstore::<8>::new();
        store.put_many(blocks).await.unwrap();

        for b in blocks {
            let cid = b.cid().unwrap();
            assert!(store.has(&cid).await.unwrap());
            let retrieved_block = store.get(&cid).await.unwrap().unwrap();
            assert_eq!(b.data(), &retrieved_block);
        }

        for b in uninserted_blocks {
            let cid = b.cid().unwrap();
            assert!(!store.has(&cid).await.unwrap());
            assert!(store.get(&cid).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn test_multiple_keyed() {
        let blocks = [[0], [1], [2], [3]];
        let cids = [
            // 4 different arbitrary CIDs
            TestBlock([0, 0, 0, 1]).cid().unwrap(),
            TestBlock([0, 0, 0, 2]).cid().unwrap(),
            TestBlock([0, 0, 0, 3]).cid().unwrap(),
            TestBlock([0, 0, 0, 4]).cid().unwrap(),
        ];
        let pairs = zip(cids, blocks);

        let store = InMemoryBlockstore::<8>::new();
        store.put_many_keyed(pairs.clone()).await.unwrap();

        for (cid, block) in pairs {
            let retrieved_block = store.get(&cid).await.unwrap().unwrap();
            assert_eq!(block.as_ref(), &retrieved_block);
        }
    }

    const TEST_CODEC: u64 = 0x0A;
    const TEST_MH_CODE: u64 = 0x0A;

    #[derive(Debug, PartialEq, Clone, Copy)]
    struct TestBlock(pub [u8; 4]);

    impl Block<8> for TestBlock {
        fn cid(&self) -> StdResult<CidGeneric<8>, CidError> {
            let mh = Multihash::wrap(TEST_MH_CODE, &self.0).unwrap();
            Ok(CidGeneric::new_v1(TEST_CODEC, mh))
        }

        fn data(&self) -> &[u8] {
            &self.0
        }
    }
}
