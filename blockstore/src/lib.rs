use async_trait::async_trait;
use cid::CidGeneric;

pub use in_memory_blockstore::InMemoryBlockstore;

mod in_memory_blockstore;

#[derive(Debug, PartialEq)]
pub enum BlockstoreError {
    CidNotFound,
    CidExists,
    CidTooLong,
}

type Result<T> = std::result::Result<T, BlockstoreError>;

#[async_trait]
pub trait Blockstore {
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Vec<u8>>;
    async fn insert<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()>;
}
