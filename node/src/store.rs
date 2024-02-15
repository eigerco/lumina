//! Primitives related to the [`ExtendedHeader`] storage.

use std::fmt::Debug;
use std::io::{self, Cursor};
use std::ops::{Bound, RangeBounds, RangeInclusive};
use std::sync::Arc;

use async_trait::async_trait;
use celestia_tendermint_proto::Protobuf;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use prost::Message;
use serde::{Deserialize, Serialize};
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

/// Sampling status for a header.
///
/// This struct persists DAS-ing information in a header store for future reference.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SamplingMetadata {
    /// Indicates whether this node was able to successfuly sample the block
    pub accepted: bool,

    /// List of CIDs used, when decision to accept or reject the header was taken. Can be used
    /// to remove associated data from Blockstore, when cleaning up the old ExtendedHeaders
    pub cids_sampled: Vec<Cid>,
}

type Result<T, E = StoreError> = std::result::Result<T, E>;

/// An asynchronous [`ExtendedHeader`] storage.
///
/// Currently it is required that all the headers are inserted to the storage
/// in order, starting from the genesis.
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

        let amount = if range.is_empty() {
            0
        } else {
            range.end() - range.start() + 1 // add one as it's inclusive
        };

        let mut headers = Vec::with_capacity(
            amount
                .try_into()
                .map_err(|_| StoreError::InvalidHeadersRange)?,
        );

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

    /// Returns height of the lowest header that wasn't sampled yet
    async fn next_unsampled_height(&self) -> Result<u64>;

    /// Sets or updates sampling result for the header.
    ///
    /// In case of update, provided CID list is appended onto the existing one, as not to lose
    /// references to previously sampled blocks.
    ///
    /// Returns next unsampled header or error, if occured
    async fn update_sampling_metadata(
        &self,
        height: u64,
        accepted: bool,
        cids: Vec<Cid>,
    ) -> Result<u64>;

    /// Gets the sampling data for the height. `Ok(None)` indicates that sampling data
    /// wasn't set in the store
    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>>;

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

/// Representation of all the errors that can occur when interacting with the [`Store`].
#[derive(Error, Debug)]
pub enum StoreError {
    /// Hash already exists in the store.
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),

    /// Height already exists in the store.
    #[error("Height {0} already exists in store")]
    HeightExists(u64),

    /// Inserted height is not following store's current head.
    #[error("Failed to append header at height {1}, current head {0}")]
    NonContinuousAppend(u64, u64),

    /// Header validation has failed.
    #[error("Failed to validate header at height {0}")]
    HeaderChecksError(u64),

    /// Header not found.
    #[error("Header not found in store")]
    NotFound,

    /// Header not found but it should be present. Store is invalid.
    #[error("Store in inconsistent state; height {0} within known range, but missing header")]
    LostHeight(u64),

    /// Hash not found but it should be present. Store is invalid.
    #[error("Store in inconsistent state; height->hash mapping exists, {0} missing")]
    LostHash(Hash),

    /// An error propagated from the [`celestia_types`].
    #[error(transparent)]
    CelestiaTypes(#[from] celestia_types::Error),

    /// Storage corrupted.
    #[error("Stored data in inconsistent state, try reseting the store: {0}")]
    StoredDataError(String),

    /// Unrecoverable error reported by the backing store.
    #[error("Persistent storage reported unrecoverable error: {0}")]
    BackingStoreError(String),

    /// An error propagated from the async executor.
    #[error("Received error from executor: {0}")]
    ExecutorError(String),

    /// An error propagated from the IO operation.
    #[error("Received io error from persistent storage: {0}")]
    IoError(#[from] io::Error),

    /// Failed to open the store.
    #[error("Error opening store: {0}")]
    OpenFailed(String),

    /// Invalid range of headers provided.
    #[error("Invalid headers range")]
    InvalidHeadersRange,
}

// impl<S> Store for Arc<S>
// where
//     S: Store + ?Sized,
// {
//     async fn get_head(&self) -> Result<ExtendedHeader> {
//         self.get_head()
//     }

//     async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
//         self.get_by_hash(hash)
//     }

//     async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
//         self.get_by_height(height)
//     }

//     async fn head_height(&self) -> Result<u64> {
//         self.head_height()
//     }

//     async fn has(&self, hash: &Hash) -> bool {
//         self.has(hash)
//     }

//     async fn has_at(&self, height: u64) -> bool {
//         self.has_at(height)
//     }

//     async fn append_single_unchecked(&self, header: ExtendedHeader) -> Result<()> {
//         self.append_single_unchecked(header)
//     }

//     async fn next_unsampled_height(&self) -> Result<u64> {
//         self.next_unsampled_height()
//     }

//     async fn update_sampling_metadata(
//         &self,
//         height: u64,
//         accepted: bool,
//         cids: Vec<Cid>,
//     ) -> Result<u64> {
//         self.update_sampling_metadata(height, accepted, cids)
//     }

//     async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
//         self.get_sampling_metadata(height)
//     }
// }

#[derive(Message)]
struct RawSamplingMetadata {
    #[prost(bool, tag = "1")]
    accepted: bool,

    #[prost(message, repeated, tag = "2")]
    cids_sampled: Vec<Vec<u8>>,
}

impl Protobuf<RawSamplingMetadata> for SamplingMetadata {}

impl TryFrom<RawSamplingMetadata> for SamplingMetadata {
    type Error = cid::Error;

    fn try_from(item: RawSamplingMetadata) -> Result<Self, Self::Error> {
        let cids_sampled = item
            .cids_sampled
            .iter()
            .map(|cid| {
                let buffer = Cursor::new(cid);
                Cid::read_bytes(buffer)
            })
            .collect::<Result<_, _>>()?;

        Ok(SamplingMetadata {
            accepted: item.accepted,
            cids_sampled,
        })
    }
}

impl From<SamplingMetadata> for RawSamplingMetadata {
    fn from(item: SamplingMetadata) -> Self {
        let cids_sampled = item.cids_sampled.iter().map(|cid| cid.to_bytes()).collect();

        RawSamplingMetadata {
            accepted: item.accepted,
            cids_sampled,
        }
    }
}

/// a helper function to convert any kind of range to the inclusive range of header heights.
fn to_headers_range(bounds: impl RangeBounds<u64>, last_index: u64) -> Result<RangeInclusive<u64>> {
    let start = match bounds.start_bound() {
        // in case of unbounded, default to the first height
        Bound::Unbounded => 1,
        // range starts after the last index or before first height
        Bound::Included(&x) if x > last_index || x == 0 => return Err(StoreError::NotFound),
        Bound::Excluded(&x) if x >= last_index => return Err(StoreError::NotFound),
        // valid start indexes
        Bound::Included(&x) => x,
        Bound::Excluded(&x) => x + 1, // can't overflow thanks to last_index check
    };
    let end = match bounds.end_bound() {
        // in case of unbounded, default to the last index
        Bound::Unbounded => last_index,
        // range ends after the last index
        Bound::Included(&x) if x > last_index => return Err(StoreError::NotFound),
        Bound::Excluded(&x) if x > last_index + 1 => return Err(StoreError::NotFound),
        // prevent the underflow later on
        Bound::Excluded(&0) => 0,
        // valid end indexes
        Bound::Included(&x) => x,
        Bound::Excluded(&x) => x - 1,
    };

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
    fn handle_ranges_ending_precisely_at_last_index() {
        let last_index = 10;

        let bounds_ending_at_last_index = [
            (Bound::Unbounded, Bound::Included(last_index)),
            (Bound::Unbounded, Bound::Excluded(last_index + 1)),
        ];

        for bound in bounds_ending_at_last_index {
            let range = to_headers_range(bound, last_index).unwrap();
            assert_eq!(*range.end(), last_index);
        }
    }

    #[test]
    fn handle_ranges_ending_after_last_index() {
        let last_index = 10;

        let bounds_ending_after_last_index = [
            (Bound::Unbounded, Bound::Included(last_index + 1)),
            (Bound::Unbounded, Bound::Excluded(last_index + 2)),
        ];

        for bound in bounds_ending_after_last_index {
            to_headers_range(bound, last_index).unwrap_err();
        }
    }

    #[test]
    fn errors_if_zero_heigth_is_included() {
        let includes_zero_height = 0..5;
        to_headers_range(includes_zero_height, 10).unwrap_err();
    }

    #[test]
    fn handle_ranges_starting_precisely_at_last_index() {
        let last_index = 10;

        let bounds_starting_at_last_index = [
            (Bound::Included(last_index), Bound::Unbounded),
            (Bound::Excluded(last_index - 1), Bound::Unbounded),
        ];

        for bound in bounds_starting_at_last_index {
            let range = to_headers_range(bound, last_index).unwrap();
            assert_eq!(*range.start(), last_index);
        }
    }

    #[test]
    fn handle_ranges_starting_after_last_index() {
        let last_index = 10;

        let bounds_starting_after_last_index = [
            (Bound::Included(last_index + 1), Bound::Unbounded),
            (Bound::Excluded(last_index), Bound::Unbounded),
        ];

        for bound in bounds_starting_after_last_index {
            to_headers_range(bound, last_index).unwrap_err();
        }
    }

    #[test]
    fn handle_ranges_that_lead_to_empty_ranges() {
        let last_index = 10;

        let bounds_leading_to_empty_range = [
            (Bound::Unbounded, Bound::Excluded(0)),
            (Bound::Included(3), Bound::Excluded(3)),
            (Bound::Included(3), Bound::Included(2)),
            (Bound::Excluded(2), Bound::Included(2)),
        ];

        for bound in bounds_leading_to_empty_range {
            assert!(to_headers_range(bound, last_index).unwrap().is_empty());
        }
    }
}
