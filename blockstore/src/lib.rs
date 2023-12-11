use async_trait::async_trait;
use cid::CidGeneric;

use crate::multihash::Block;

pub use crate::in_memory_blockstore::InMemoryBlockstore;

mod in_memory_blockstore;
pub mod multihash;

#[derive(Debug, PartialEq)]
pub enum BlockstoreError {
    CidExists,
    CidTooLong,
}

type Result<T> = std::result::Result<T, BlockstoreError>;

#[async_trait]
pub trait Blockstore {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>>;
    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()>;

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        Ok(self.get(cid).await?.is_some())
    }

    async fn put<const S: usize, B>(&self, block: B) -> Result<()>
    where
        B: Block<S>,
    {
        let cid = block.cid_v1()?;
        self.put_keyed(&cid, block.as_ref()).await
    }

    async fn put_many<const S: usize, B, I>(&self, blocks: I) -> Result<()>
    where
        B: Block<S>,
        I: IntoIterator<Item = B> + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        for b in blocks {
            let cid = b.cid_v1()?;
            self.put_keyed(&cid, b.as_ref()).await?;
        }
        Ok(())
    }

    async fn put_many_keyed<const S: usize, D, I>(&self, blocks: I) -> Result<()>
    where
        D: AsRef<[u8]> + Send + Sync,
        I: IntoIterator<Item = (CidGeneric<S>, D)> + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        for (cid, block) in blocks {
            self.put_keyed(&cid, block.as_ref()).await?;
        }
        Ok(())
    }
}
