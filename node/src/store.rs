use std::fmt::Debug;
use std::io;
use std::ops::{Bound, RangeBounds, RangeInclusive};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use thiserror::Error;

pub use in_memory_store::InMemoryStore;
#[cfg(target_arch = "wasm32")]
pub use indexed_db_store::IndexedDbStore;
#[cfg(not(target_arch = "wasm32"))]
pub use sled_store::SledStore;

mod in_memory_store;
#[cfg(target_arch = "wasm32")]
mod indexed_db_store;
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

    /// Returns the headers from the given heights range.
    ///
    /// If start of the range is unbounded, the first returned header will be of height 1.
    /// height.
    /// If end of the range is unbounded, the last returned header will be the last header in the
    /// store.
    ///
    /// # Errors
    ///
    /// If range contains a height of a header that is not found in the store or [`RangeBounds`]
    /// cannot be converted to a valid range.
    async fn get_range<R>(&self, range: R) -> Result<Vec<ExtendedHeader>>
    where
        R: RangeBounds<u64> + Send,
    {
        let head_height = self.head_height().await?;
        let range = to_headers_range(range, head_height)?;
        let mut headers = Vec::with_capacity(range.clone().count());

        for height in range {
            let header = self.get_by_height(height).await?;
            headers.push(header);
        }

        Ok(headers)
    }

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

    #[error("Error opening store: {0}")]
    OpenFailed(String),

    #[error("Invalid headers range")]
    InvalidHeadersRange,
}

/// a helper function to convert any kind of range to the inclusive range of header heights.
fn to_headers_range(bounds: impl RangeBounds<u64>, last_index: u64) -> Result<RangeInclusive<u64>> {
    let start = match bounds.start_bound() {
        Bound::Unbounded => 1,
        Bound::Included(&x) => x,
        Bound::Excluded(&u64::MAX) => return Err(StoreError::InvalidHeadersRange),
        Bound::Excluded(&x) => x + 1,
    };
    let end = match bounds.end_bound() {
        Bound::Unbounded => last_index,
        Bound::Included(&x) => x,
        Bound::Excluded(&0) => return Err(StoreError::InvalidHeadersRange),
        Bound::Excluded(&x) => x - 1,
    };
    if start < 1 {
        return Err(StoreError::NotFound);
    }
    Ok(start..=end)
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use super::to_headers_range;

    #[test]
    fn converts_bounded_ranges() {
        assert_eq!(1..=15, to_headers_range(1..16, 100).unwrap());
        assert_eq!(1..=15, to_headers_range(1..=15, 100).unwrap());
        assert_eq!(300..=400, to_headers_range(300..401, 500).unwrap());
        assert_eq!(300..=400, to_headers_range(300..=400, 500).unwrap());
    }

    #[test]
    fn starts_from_one_when_unbounded_start() {
        assert_eq!(&1, to_headers_range(..=10, 100).unwrap().start());
        assert_eq!(&1, to_headers_range(..10, 100).unwrap().start());
        assert_eq!(&1, to_headers_range(.., 100).unwrap().start());
    }

    #[test]
    fn ends_on_last_index_when_unbounded_end() {
        assert_eq!(&10, to_headers_range(1.., 10).unwrap().end());
        assert_eq!(&11, to_headers_range(1.., 11).unwrap().end());
        assert_eq!(&10, to_headers_range(.., 10).unwrap().end());
    }

    #[test]
    fn handles_edge_cases() {
        let includes_zero_height = 0..5;
        to_headers_range(includes_zero_height, 10).unwrap_err();

        let start_overflow = (Bound::Excluded(u64::MAX), Bound::Unbounded);
        to_headers_range(start_overflow, 10).unwrap_err();

        let end_underflow = (Bound::Unbounded, Bound::Excluded(0));
        to_headers_range(end_underflow, 10).unwrap_err();
    }
}
