use cid::CidGeneric;
use multihash::Multihash;

use crate::block::{Block, CidError};

const TEST_CODEC: u64 = 0x0A;
const TEST_MH_CODE: u64 = 0x0A;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct TestBlock(pub [u8; 4]);

impl Block<8> for TestBlock {
    fn cid(&self) -> Result<CidGeneric<8>, CidError> {
        let mh = Multihash::wrap(TEST_MH_CODE, &self.0).unwrap();
        Ok(CidGeneric::new_v1(TEST_CODEC, mh))
    }

    fn data(&self) -> &[u8] {
        &self.0
    }
}

pub fn cid_v1<const S: usize>(data: impl AsRef<[u8]>) -> CidGeneric<S> {
    CidGeneric::new_v1(1, Multihash::wrap(1, data.as_ref()).unwrap())
}

macro_rules! blockstore_tests {
    ($create_store:ident, $test:path) => {
        #[$test]
        async fn test_insert_get() {
            let store = $create_store::<64>().await;

            let cid = crate::test_utils::cid_v1::<64>(b"1");
            let data = b"3";
            crate::Blockstore::put_keyed(&store, &cid, data)
                .await
                .unwrap();

            let retrieved_data = crate::Blockstore::get(&store, &cid).await.unwrap().unwrap();
            assert_eq!(&retrieved_data, data);
            assert!(crate::Blockstore::has(&store, &cid).await.unwrap());

            let another_cid = crate::test_utils::cid_v1::<64>(b"2");
            let missing_block = crate::Blockstore::get(&store, &another_cid).await.unwrap();
            assert_eq!(missing_block, None);
            assert!(!crate::Blockstore::has(&store, &another_cid).await.unwrap());
        }

        #[$test]
        async fn test_duplicate_insert() {
            let store = $create_store::<64>().await;

            let cid0 = crate::test_utils::cid_v1::<64>(b"1");
            let cid1 = crate::test_utils::cid_v1::<64>(b"2");

            crate::Blockstore::put_keyed(&store, &cid0, b"1")
                .await
                .unwrap();
            crate::Blockstore::put_keyed(&store, &cid1, b"2")
                .await
                .unwrap();

            let insert_err = crate::Blockstore::put_keyed(&store, &cid1, b"3")
                .await
                .unwrap_err();
            assert!(matches!(insert_err, crate::BlockstoreError::CidExists));
        }

        #[$test]
        async fn different_cid_size() {
            let store = $create_store::<128>().await;

            let cid0 = crate::test_utils::cid_v1::<32>(b"1");
            let cid1 = crate::test_utils::cid_v1::<64>(b"1");
            let cid2 = crate::test_utils::cid_v1::<128>(b"1");
            let data = b"2";

            crate::Blockstore::put_keyed(&store, &cid0, data)
                .await
                .unwrap();

            let received = crate::Blockstore::get(&store, &cid0)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&received, data);
            let received = crate::Blockstore::get(&store, &cid1)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&received, data);
            let received = crate::Blockstore::get(&store, &cid2)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&received, data);
        }

        #[$test]
        async fn too_large_cid() {
            let store = $create_store::<8>().await;

            let small_cid = crate::test_utils::cid_v1::<64>([1u8; 8]);
            let big_cid = crate::test_utils::cid_v1::<64>([1u8; 64]);

            crate::Blockstore::put_keyed(&store, &small_cid, b"1")
                .await
                .unwrap();
            let put_err = crate::Blockstore::put_keyed(&store, &big_cid, b"1")
                .await
                .unwrap_err();
            assert!(matches!(put_err, crate::BlockstoreError::CidTooLong));

            crate::Blockstore::get(&store, &small_cid).await.unwrap();
            let get_err = crate::Blockstore::get(&store, &big_cid).await.unwrap_err();
            assert!(matches!(get_err, crate::BlockstoreError::CidTooLong));

            crate::Blockstore::has(&store, &small_cid).await.unwrap();
            let has_err = crate::Blockstore::has(&store, &big_cid).await.unwrap_err();
            assert!(matches!(has_err, crate::BlockstoreError::CidTooLong));
        }

        #[$test]
        async fn test_block_insert() {
            use crate::block::Block;

            let store = $create_store::<8>().await;

            let block = crate::test_utils::TestBlock([0, 1, 2, 3]);

            crate::Blockstore::put(&store, block).await.unwrap();
            let retrieved_block = crate::Blockstore::get(&store, &block.cid().unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(block.data(), &retrieved_block);
        }

        #[$test]
        async fn test_multiple_blocks_insert() {
            use crate::block::Block;

            let store = $create_store::<8>().await;

            let blocks = [
                crate::test_utils::TestBlock([0, 0, 0, 0]),
                crate::test_utils::TestBlock([0, 0, 0, 1]),
                crate::test_utils::TestBlock([0, 0, 1, 0]),
                crate::test_utils::TestBlock([0, 0, 1, 1]),
                crate::test_utils::TestBlock([0, 1, 0, 0]),
                crate::test_utils::TestBlock([0, 1, 0, 1]),
                crate::test_utils::TestBlock([0, 1, 1, 0]),
                crate::test_utils::TestBlock([0, 1, 1, 1]),
            ];
            let uninserted_blocks = [
                crate::test_utils::TestBlock([1, 0, 0, 0]),
                crate::test_utils::TestBlock([1, 0, 0, 1]),
                crate::test_utils::TestBlock([1, 0, 1, 0]),
                crate::test_utils::TestBlock([1, 1, 0, 1]),
            ];

            crate::Blockstore::put_many(&store, blocks).await.unwrap();

            for b in blocks {
                let cid = b.cid().unwrap();
                assert!(crate::Blockstore::has(&store, &cid).await.unwrap());
                let retrieved_block = crate::Blockstore::get(&store, &cid).await.unwrap().unwrap();
                assert_eq!(b.data(), &retrieved_block);
            }

            for b in uninserted_blocks {
                let cid = b.cid().unwrap();
                assert!(!crate::Blockstore::has(&store, &cid).await.unwrap());
                assert!(crate::Blockstore::get(&store, &cid)
                    .await
                    .unwrap()
                    .is_none());
            }
        }

        #[$test]
        async fn test_multiple_keyed() {
            use crate::block::Block;

            let store = $create_store::<8>().await;

            let blocks = [[0], [1], [2], [3]];
            let cids = [
                // 4 different arbitrary CIDs
                crate::test_utils::TestBlock([0, 0, 0, 1]).cid().unwrap(),
                crate::test_utils::TestBlock([0, 0, 0, 2]).cid().unwrap(),
                crate::test_utils::TestBlock([0, 0, 0, 3]).cid().unwrap(),
                crate::test_utils::TestBlock([0, 0, 0, 4]).cid().unwrap(),
            ];
            let pairs = std::iter::zip(cids, blocks);

            crate::Blockstore::put_many_keyed(&store, pairs.clone())
                .await
                .unwrap();

            for (cid, block) in pairs {
                let retrieved_block = crate::Blockstore::get(&store, &cid).await.unwrap().unwrap();
                assert_eq!(block.as_ref(), &retrieved_block);
            }
        }
    };
}

pub(crate) use blockstore_tests;
