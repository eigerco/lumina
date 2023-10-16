use std::fmt::Debug;
use std::io;

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use thiserror::Error;

pub use in_memory_store::InMemoryStore;
#[cfg(not(target_arch = "wasm32"))]
pub use sled_store::SledStore;

mod in_memory_store;
#[cfg(not(target_arch = "wasm32"))]
mod sled_store;

use crate::utils::validate_headers;

type Result<T, E = StoreError> = std::result::Result<T, E>;

#[async_trait]
pub trait Store: Send + Sync + Debug {
    /// Returns the [`ExtendedHeader`] with the highest height.
    async fn get_head(&self) -> Result<ExtendedHeader>;

    /// Returns the header of a specific hash.
    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader>;

    /// Returns the header of a specific height.
    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader>;

    /// Returns the highest known height.
    async fn head_height(&self) -> Result<u64>;

    /// Returns true if hash exists in the store.
    async fn has(&self, hash: &Hash) -> bool;

    /// Returns true if height exists in the store.
    async fn has_at(&self, height: u64) -> bool;

    /// Append single header maintaining continuity from the genesis to the head.
    ///
    /// # Note
    ///
    /// This method does not validate or verify that `header` is indeed correct.
    async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()>;

    /// Append a range of headers maintaining continuity from the genesis to the head.
    ///
    /// # Note
    ///
    /// This method does not validate or verify that `headers` are indeed correct.
    async fn append_unchecked(&self, headers: Vec<ExtendedHeader>) -> Result<()> {
        for header in headers.into_iter() {
            self.append_single_unchecked(header).await?;
        }

        Ok(())
    }

    /// Append single header maintaining continuity from the genesis to the head.
    async fn append_single(&self, header: ExtendedHeader) -> Result<()> {
        header.validate()?;

        match self.get_head().await {
            Ok(head) => {
                head.verify(&header)?;
            }
            // Empty store, we can not verify
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e),
        }

        self.append_single_unchecked(header).await
    }

    /// Append a range of headers maintaining continuity from the genesis to the head.
    async fn append(&self, headers: Vec<ExtendedHeader>) -> Result<()> {
        validate_headers(&headers).await?;

        match self.get_head().await {
            Ok(head) => {
                head.verify_adjacent_range(&headers)?;
            }
            // Empty store, we can not verify
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e),
        }

        self.append_unchecked(headers).await
    }
}

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),

    #[error("Height {0} already exists in store")]
    HeightExists(u64),

    #[error("Failed to append header at height {1}, current head {0}")]
    NonContinuousAppend(u64, u64),

    #[error("Failed to validate header at height {0}")]
    HeaderChecksError(u64),

    #[error("Header not found in store")]
    NotFound,

    #[error("Store in inconsistent state; height {0} within known range, but missing header")]
    LostHeight(u64),

    #[error("Store in inconsistent state; height->hash mapping exists, {0} missing")]
    LostHash(Hash),

    #[error(transparent)]
    CelestiaTypes(#[from] celestia_types::Error),

    #[error("Stored data in inconsistent state, try reseting the store: {0}")]
    StoredDataError(String),

    #[error("Persistent storage reported unrecoverable error: {0}")]
    BackingStoreError(String),

    #[error("Received error from executor: {0}")]
    ExecutorError(String),

    #[error("Received io error from persistent storage: {0}")]
    IoError(#[from] io::Error),
}
