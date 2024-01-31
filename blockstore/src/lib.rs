#![doc = include_str!("../README.md")]

use cid::CidGeneric;
use multihash::Multihash;
use thiserror::Error;

use crate::block::{Block, CidError};

/// Utilities related to computing CID for the inserted data
pub mod block;
mod in_memory_blockstore;
#[cfg(feature = "lru")]
mod lru_blockstore;

pub use crate::in_memory_blockstore::InMemoryBlockstore;
#[cfg(feature = "lru")]
#[cfg_attr(docs_rs, doc(cfg(feature = "lru")))]
pub use crate::lru_blockstore::LruBlockstore;

/// Error returned when performing operations on [`Blockstore`]
#[derive(Debug, PartialEq, Error)]
pub enum BlockstoreError {
    /// Provided CID already exists in blockstore when trying to insert it
    #[error("CID already exists in the store")]
    CidExists,

    /// Provided CID is longer than max length supported by the blockstore
    #[error("CID length longer that max allowed by the store")]
    CidTooLong,

    /// Error occured when trying to compute CID.
    #[error("Error generating CID: {0}")]
    CidError(#[from] CidError),
}

type Result<T> = std::result::Result<T, BlockstoreError>;

/// An IPLD blockstore capable of holding arbitrary data indexed by CID.
///
/// Implementations can impose limit on supported CID length, and any operations on longer CIDs
/// will fail with [`CidTooLong`].
///
/// [`CidTooLong`]: BlockstoreError::CidTooLong
#[cfg_attr(not(docs_rs), async_trait::async_trait)]
pub trait Blockstore {
    /// Gets the block from the blockstore
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>>;

    /// Inserts the data with pre-computed CID.
    /// Use [`put`], if you want CID to be computed.
    ///
    /// [`put`]: Blockstore::put
    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()>;

    /// Checks whether blockstore has block for provided CID
    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        Ok(self.get(cid).await?.is_some())
    }

    /// Inserts the data into the blockstore, computing CID using [`Block`] trait.
    async fn put<const S: usize, B>(&self, block: B) -> Result<()>
    where
        B: Block<S>,
    {
        let cid = block.cid()?;
        self.put_keyed(&cid, block.data()).await
    }

    /// Inserts multiple blocks into the blockstore computing their CID
    /// If CID computation, or insert itself fails, error is returned and subsequent items are also
    /// skipped.
    async fn put_many<const S: usize, B, I>(&self, blocks: I) -> Result<()>
    where
        B: Block<S>,
        I: IntoIterator<Item = B> + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        for b in blocks {
            let cid = b.cid()?;
            self.put_keyed(&cid, b.data()).await?;
        }
        Ok(())
    }

    /// Inserts multiple blocks with pre-computed CID into the blockstore.
    /// If any put from the list fails, error is returned and subsequent items are also skipped.
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

pub(crate) fn convert_cid<const S: usize, const NEW_S: usize>(
    cid: &CidGeneric<S>,
) -> Result<CidGeneric<NEW_S>> {
    let hash = Multihash::<NEW_S>::wrap(cid.hash().code(), cid.hash().digest())
        .map_err(|_| BlockstoreError::CidTooLong)?;

    // Safe to unwrap because check was done from previous construction.
    let cid = CidGeneric::new(cid.version(), cid.codec(), hash).expect("malformed cid");

    Ok(cid)
}
