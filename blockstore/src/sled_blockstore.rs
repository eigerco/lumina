use std::io;
use std::sync::Arc;

use cid::CidGeneric;
use sled::{Db, Error as SledError, Tree};
use tokio::task::spawn_blocking;
use tokio::task::JoinError;

use crate::{convert_cid, Blockstore, BlockstoreError, Result};

const BLOCKS_TREE_ID: &[u8] = b"BLOCKSTORE.BLOCKS";

/// A [`Blockstore`] implementation backed by a [`sled`] database.
#[derive(Debug)]
pub struct SledBlockstore<const MAX_MULTIHASH_SIZE: usize> {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    _db: Db,
    blocks: Tree,
}

impl<const MAX_MULTIHASH_SIZE: usize> SledBlockstore<MAX_MULTIHASH_SIZE> {
    /// Create or open a [`SledBlockstore`] in a given sled [`Db`].
    ///
    /// # Example
    /// ```
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// use blockstore::SledBlockstore;
    /// use tokio::task::spawn_blocking;
    ///
    /// let db = spawn_blocking(|| sled::open("path/to/db")).await??;
    /// let blockstore = SledBlockstore::<64>::new(db).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(db: Db) -> Result<Self> {
        spawn_blocking(move || {
            let blocks = db.open_tree(BLOCKS_TREE_ID)?;

            Ok(Self {
                inner: Arc::new(Inner { _db: db, blocks }),
            })
        })
        .await?
    }

    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let key = cid.to_bytes();
            Ok(inner.blocks.get(key)?.map(|bytes| bytes.to_vec()))
        })
        .await?
    }

    async fn put<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let inner = self.inner.clone();
        let data = data.to_vec();

        spawn_blocking(move || {
            let key = cid.to_bytes();
            inner
                .blocks
                .compare_and_swap(key, None as Option<&[u8]>, Some(data))?
                .or(Err(BlockstoreError::CidExists))
        })
        .await?
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        let cid = convert_cid::<S, MAX_MULTIHASH_SIZE>(cid)?;
        let inner = self.inner.clone();

        spawn_blocking(move || {
            let key = cid.to_bytes();
            Ok(inner.blocks.contains_key(key)?)
        })
        .await?
    }
}

#[cfg_attr(not(docs_rs), async_trait::async_trait)]
impl<const MAX_MULTIHASH_SIZE: usize> Blockstore for SledBlockstore<MAX_MULTIHASH_SIZE> {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        self.get(cid).await
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        self.put(cid, data).await
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        self.has(cid).await
    }
}

// divide errors into recoverable and not avoiding directly relying on passing sled types
impl From<SledError> for BlockstoreError {
    fn from(error: SledError) -> BlockstoreError {
        match error {
            e @ SledError::CollectionNotFound(_) | e @ SledError::Corruption { .. } => {
                BlockstoreError::StorageCorrupted(e.to_string())
            }
            e @ SledError::Unsupported(_) | e @ SledError::ReportableBug(_) => {
                BlockstoreError::BackingStoreError(e.to_string())
            }
            SledError::Io(e) => e.into(),
        }
    }
}

impl From<JoinError> for BlockstoreError {
    fn from(error: JoinError) -> BlockstoreError {
        io::Error::from(error).into()
    }
}

#[cfg(test)]
mod tests {
    use cid::Cid;
    use multihash::Multihash;

    use super::*;

    #[tokio::test]
    async fn test_insert_get() {
        let cid = cid_v1::<64>(b"1");
        let data = b"3";

        let store = temp_blockstore::<64>().await;
        store.put_keyed(&cid, data).await.unwrap();

        let retrieved_data = store.get(&cid).await.unwrap().unwrap();
        assert_eq!(&retrieved_data, data);
        assert!(store.has(&cid).await.unwrap());

        let another_cid = Cid::default();
        let missing_block = store.get(&another_cid).await.unwrap();
        assert_eq!(missing_block, None);
        assert!(!store.has(&another_cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_duplicate_insert() {
        let cid0 = cid_v1::<64>(b"1");
        let cid1 = cid_v1::<64>(b"2");

        let store = temp_blockstore::<64>().await;

        store.put_keyed(&cid0, b"1").await.unwrap();
        store.put_keyed(&cid1, b"2").await.unwrap();

        let insert_err = store.put_keyed(&cid1, b"3").await.unwrap_err();
        assert!(matches!(insert_err, BlockstoreError::CidExists));
    }

    #[tokio::test]
    async fn different_cid_size() {
        let cid0 = cid_v1::<32>(b"1");
        let cid1 = cid_v1::<64>(b"1");
        let cid2 = cid_v1::<128>(b"1");
        let data = b"2";

        let store = temp_blockstore::<128>().await;
        store.put_keyed(&cid0, data).await.unwrap();

        let received = store.get(&cid0).await.unwrap().unwrap();
        assert_eq!(&received, data);
        let received = store.get(&cid1).await.unwrap().unwrap();
        assert_eq!(&received, data);
        let received = store.get(&cid2).await.unwrap().unwrap();
        assert_eq!(&received, data);
    }

    #[tokio::test]
    async fn too_large_cid() {
        let small_cid = cid_v1::<64>([1u8; 8]);
        let big_cid = cid_v1::<64>([1u8; 64]);

        let store = temp_blockstore::<8>().await;

        store.put_keyed(&small_cid, b"1").await.unwrap();
        let put_err = store.put_keyed(&big_cid, b"1").await.unwrap_err();
        assert!(matches!(put_err, BlockstoreError::CidTooLong));

        store.get(&small_cid).await.unwrap();
        let get_err = store.get(&big_cid).await.unwrap_err();
        assert!(matches!(get_err, BlockstoreError::CidTooLong));

        store.has(&small_cid).await.unwrap();
        let has_err = store.has(&big_cid).await.unwrap_err();
        assert!(matches!(has_err, BlockstoreError::CidTooLong));
    }

    fn cid_v1<const S: usize>(data: impl AsRef<[u8]>) -> CidGeneric<S> {
        CidGeneric::new_v1(1, Multihash::wrap(1, data.as_ref()).unwrap())
    }

    async fn temp_blockstore<const SIZE: usize>() -> SledBlockstore<SIZE> {
        let test_dir = tempfile::TempDir::with_prefix("sled-blockstore-test")
            .unwrap()
            .into_path();

        let db = sled::Config::default()
            .path(test_dir)
            .temporary(true)
            .create_new(true)
            .open()
            .unwrap();

        SledBlockstore::new(db).await.unwrap()
    }
}
