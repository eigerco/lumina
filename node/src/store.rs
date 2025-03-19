//! Primitives related to the [`ExtendedHeader`] storage.

use std::convert::Infallible;
use std::fmt::{Debug, Display};
use std::io::Cursor;
use std::ops::{Bound, RangeBounds, RangeInclusive};

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;
use prost::Message;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;
use thiserror::Error;
#[cfg(all(feature = "wasm-bindgen", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

pub use crate::block_ranges::{BlockRange, BlockRanges, BlockRangesError};
pub use crate::store::either_store::EitherStore;
pub use crate::store::utils::VerifiedExtendedHeaders;

pub use in_memory_store::InMemoryStore;
#[cfg(target_arch = "wasm32")]
pub use indexed_db_store::IndexedDbStore;
#[cfg(not(target_arch = "wasm32"))]
pub use redb_store::RedbStore;

mod either_store;
mod in_memory_store;
#[cfg(target_arch = "wasm32")]
mod indexed_db_store;
#[cfg(not(target_arch = "wasm32"))]
mod redb_store;

pub(crate) mod utils;

/// Sampling metadata for a block.
///
/// This struct persists DAS-ing information in a header store for future reference.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm-bindgen", target_arch = "wasm32"), wasm_bindgen)]
pub struct SamplingMetadata {
    /// Indicates whether this node was able to successfuly sample the block
    pub status: SamplingStatus,

    /// List of CIDs used while sampling. Can be used to remove associated data
    /// from Blockstore, when cleaning up the old ExtendedHeaders
    #[cfg_attr(
        all(feature = "wasm-bindgen", target_arch = "wasm32"),
        wasm_bindgen(skip)
    )]
    pub cids: Vec<Cid>,
}

/// Sampling status for a block.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm-bindgen", target_arch = "wasm32"), wasm_bindgen)]
pub enum SamplingStatus {
    /// Sampling is not done.
    #[default]
    Unknown,
    /// Sampling is done and block is accepted.
    Accepted,
    /// Sampling is done and block is rejected.
    Rejected,
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

    /// Returns when new head is available in the `Store`.
    async fn wait_new_head(&self) -> u64;

    /// Returns when `height` is available in the `Store`.
    async fn wait_height(&self, height: u64) -> Result<()>;

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

        let mut headers = Vec::with_capacity(amount.try_into().unwrap_or(usize::MAX));

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

    /// Sets or updates sampling result for the header.
    ///
    /// In case of update, provided CID list is appended onto the existing one, as not to lose
    /// references to previously sampled blocks.
    async fn update_sampling_metadata(
        &self,
        height: u64,
        status: SamplingStatus,
        cids: Vec<Cid>,
    ) -> Result<()>;

    /// Gets the sampling metadata for the height.
    ///
    /// `Err(StoreError::NotFound)` indicates that both header **and** sampling metadata for the requested
    /// height are not in the store.
    ///
    /// `Ok(None)` indicates that header is in the store but sampling metadata is not set yet.
    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>>;

    /// Insert a range of headers into the store.
    ///
    /// New insertion should pass all the constraints in [`BlockRanges::check_insertion_constraints`],
    /// additionaly it should be [`ExtendedHeader::verify`]ed against neighbor headers.
    async fn insert<R>(&self, headers: R) -> Result<()>
    where
        R: TryInto<VerifiedExtendedHeaders> + Send,
        <R as TryInto<VerifiedExtendedHeaders>>::Error: Display;

    /// Returns a list of header ranges currenty held in store.
    async fn get_stored_header_ranges(&self) -> Result<BlockRanges>;

    /// Returns a list of accepted sampling ranges currently held in store.
    async fn get_accepted_sampling_ranges(&self) -> Result<BlockRanges>;

    /// Remove header with given height from the store.
    async fn remove_height(&self, height: u64) -> Result<()>;

    /// Close store.
    async fn close(self) -> Result<()>;
}

/// Representation of all the errors that can occur when interacting with the [`Store`].
#[derive(Error, Debug)]
pub enum StoreError {
    /// Header not found.
    #[error("Header not found in store")]
    NotFound,

    /// Non-fatal error during insertion.
    #[error("Insertion failed: {0}")]
    InsertionFailed(#[from] StoreInsertionError),

    /// Storage corrupted.
    #[error("Stored data are inconsistent or invalid, try reseting the store: {0}")]
    StoredDataError(String),

    /// Unrecoverable error reported by the database.
    #[error("Database reported unrecoverable error: {0}")]
    FatalDatabaseError(String),

    /// An error propagated from the async executor.
    #[error("Received error from executor: {0}")]
    ExecutorError(String),

    /// Failed to open the store.
    #[error("Error opening store: {0}")]
    OpenFailed(String),
}

/// Store insersion non-fatal errors.
#[derive(Error, Debug)]
pub enum StoreInsertionError {
    /// Provided headers failed verification.
    #[error("Provided headers failed verification: {0}")]
    HeadersVerificationFailed(String),

    /// Provided headers cannot be appended on existing headers of the store.
    #[error("Provided headers failed to be verified with existing neighbors: {0}")]
    NeighborsVerificationFailed(String),

    /// Store containts are not met.
    #[error("Contraints not met: {0}")]
    ContraintsNotMet(BlockRangesError),

    // TODO: Same hash for two different heights is not really possible
    // and `ExtendedHeader::validate` would return an error.
    // Remove this when a type-safe validation is implemented.
    /// Hash already exists in the store.
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),
}

impl StoreError {
    /// Returns `true` if an error is fatal.
    pub(crate) fn is_fatal(&self) -> bool {
        match self {
            StoreError::StoredDataError(_)
            | StoreError::FatalDatabaseError(_)
            | StoreError::ExecutorError(_)
            | StoreError::OpenFailed(_) => true,
            StoreError::NotFound | StoreError::InsertionFailed(_) => false,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<tokio::task::JoinError> for StoreError {
    fn from(error: tokio::task::JoinError) -> StoreError {
        StoreError::ExecutorError(error.to_string())
    }
}

// Needed for `Into<VerifiedExtendedHeaders>`
impl From<Infallible> for StoreError {
    fn from(_: Infallible) -> Self {
        // Infallible should not be possible to construct
        unreachable!("Infallible failed")
    }
}

#[cfg(all(feature = "wasm-bindgen", target_arch = "wasm32"))]
#[wasm_bindgen]
impl SamplingMetadata {
    /// Return Array of cids
    #[wasm_bindgen(getter)]
    pub fn cids(&self) -> Vec<js_sys::Uint8Array> {
        self.cids
            .iter()
            .map(|cid| js_sys::Uint8Array::from(cid.to_bytes().as_ref()))
            .collect()
    }
}

#[derive(Message)]
struct RawSamplingMetadata {
    #[prost(bool, tag = "1")]
    accepted: bool,

    #[prost(message, repeated, tag = "2")]
    cids: Vec<Vec<u8>>,

    #[prost(bool, tag = "3")]
    unknown: bool,
}

impl Protobuf<RawSamplingMetadata> for SamplingMetadata {}

impl TryFrom<RawSamplingMetadata> for SamplingMetadata {
    type Error = cid::Error;

    fn try_from(item: RawSamplingMetadata) -> Result<Self, Self::Error> {
        let status = if item.unknown {
            SamplingStatus::Unknown
        } else if item.accepted {
            SamplingStatus::Accepted
        } else {
            SamplingStatus::Rejected
        };

        let cids = item
            .cids
            .iter()
            .map(|cid| {
                let buffer = Cursor::new(cid);
                Cid::read_bytes(buffer)
            })
            .collect::<Result<_, _>>()?;

        Ok(SamplingMetadata { status, cids })
    }
}

impl From<SamplingMetadata> for RawSamplingMetadata {
    fn from(item: SamplingMetadata) -> Self {
        let cids = item.cids.iter().map(|cid| cid.to_bytes()).collect();

        let (accepted, unknown) = match item.status {
            SamplingStatus::Unknown => (false, true),
            SamplingStatus::Accepted => (true, false),
            SamplingStatus::Rejected => (false, false),
        };

        RawSamplingMetadata {
            accepted,
            unknown,
            cids,
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
    use super::*;
    use crate::test_utils::ExtendedHeaderGeneratorExt;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::Height;
    use lumina_utils::test_utils::async_test as test;
    use rstest::rstest;

    // rstest only supports attributes which last segment is `test`
    // https://docs.rs/rstest/0.18.2/rstest/attr.rstest.html#inject-test-attribute
    use crate::test_utils::new_block_ranges;

    #[test]
    async fn converts_bounded_ranges() {
        assert_eq!(1..=15, to_headers_range(1..16, 100).unwrap());
        assert_eq!(1..=15, to_headers_range(1..=15, 100).unwrap());
        assert_eq!(300..=400, to_headers_range(300..401, 500).unwrap());
        assert_eq!(300..=400, to_headers_range(300..=400, 500).unwrap());
    }

    #[test]
    async fn starts_from_one_when_unbounded_start() {
        assert_eq!(&1, to_headers_range(..=10, 100).unwrap().start());
        assert_eq!(&1, to_headers_range(..10, 100).unwrap().start());
        assert_eq!(&1, to_headers_range(.., 100).unwrap().start());
    }

    #[test]
    async fn ends_on_last_index_when_unbounded_end() {
        assert_eq!(&10, to_headers_range(1.., 10).unwrap().end());
        assert_eq!(&11, to_headers_range(1.., 11).unwrap().end());
        assert_eq!(&10, to_headers_range(.., 10).unwrap().end());
    }

    #[test]
    async fn handle_ranges_ending_precisely_at_last_index() {
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
    async fn handle_ranges_ending_after_last_index() {
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
    async fn errors_if_zero_heigth_is_included() {
        let includes_zero_height = 0..5;
        to_headers_range(includes_zero_height, 10).unwrap_err();
    }

    #[test]
    async fn handle_ranges_starting_precisely_at_last_index() {
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
    async fn handle_ranges_starting_after_last_index() {
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
    async fn handle_ranges_that_lead_to_empty_ranges() {
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

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_contains_height<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        fill_store(&mut s, 2).await;

        assert!(!s.has_at(0).await);
        assert!(s.has_at(1).await);
        assert!(s.has_at(2).await);
        assert!(!s.has_at(3).await);
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_empty_store<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        assert!(matches!(s.head_height().await, Err(StoreError::NotFound)));
        assert!(matches!(s.get_head().await, Err(StoreError::NotFound)));
        assert!(matches!(
            s.get_by_height(1).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            s.get_by_hash(&Hash::Sha256([0; 32])).await,
            Err(StoreError::NotFound)
        ));
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_read_write<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut gen = ExtendedHeaderGenerator::new();

        let header = gen.next();

        s.insert(header.clone()).await.unwrap();
        assert_eq!(s.head_height().await.unwrap(), 1);
        assert_eq!(s.get_head().await.unwrap(), header);
        assert_eq!(s.get_by_height(1).await.unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_pregenerated_data<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        fill_store(&mut s, 100).await;

        assert_eq!(s.head_height().await.unwrap(), 100);
        let head = s.get_head().await.unwrap();
        assert_eq!(s.get_by_height(100).await.unwrap(), head);
        assert!(matches!(
            s.get_by_height(101).await,
            Err(StoreError::NotFound)
        ));

        let header = s.get_by_height(54).await.unwrap();
        assert_eq!(s.get_by_hash(&header.hash()).await.unwrap(), header);
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_duplicate_insert<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        let mut gen = fill_store(&mut s, 100).await;

        let header101 = gen.next();
        s.insert(header101.clone()).await.unwrap();

        let error = match s.insert(header101).await {
            Err(StoreError::InsertionFailed(StoreInsertionError::ContraintsNotMet(e))) => e,
            res => panic!("Invalid result: {res:?}"),
        };

        assert_eq!(
            error,
            BlockRangesError::BlockRangeOverlap(101..=101, 101..=101)
        );
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_overwrite_height<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        let gen = fill_store(&mut s, 100).await;

        // Height 30 with different hash
        let header29 = s.get_by_height(29).await.unwrap();
        let header30 = gen.next_of(&header29);

        let error = match s.insert(header30).await {
            Err(StoreError::InsertionFailed(StoreInsertionError::ContraintsNotMet(e))) => e,
            res => panic!("Invalid result: {res:?}"),
        };
        assert_eq!(error, BlockRangesError::BlockRangeOverlap(30..=30, 30..=30));
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_overwrite_hash<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        fill_store(&mut s, 100).await;

        let mut dup_header = s.get_by_height(99).await.unwrap();
        dup_header.header.height = Height::from(102u32);

        assert!(matches!(
            s.insert(dup_header).await,
            Err(StoreError::InsertionFailed(
                StoreInsertionError::HashExists(_)
            ))
        ));
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_append_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        let mut gen = fill_store(&mut s, 10).await;

        s.insert(gen.next_many_verified(4)).await.unwrap();
        s.get_by_height(14).await.unwrap();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_fill_range_gap<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        let mut gen = fill_store(&mut s, 10).await;

        // height 11
        let skipped = gen.next();
        // height 12
        let upcoming_head = gen.next();

        s.insert(upcoming_head).await.unwrap();
        s.insert(skipped).await.unwrap();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_fill_range_gap_with_invalid_header<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut s = s;
        let mut gen = fill_store(&mut s, 10).await;

        let mut gen_prime = gen.fork();
        // height 11
        let _skipped = gen.next();
        let another_chain = gen_prime.next();
        // height 12
        let upcoming_head = gen.next();

        s.insert(upcoming_head).await.unwrap();
        assert!(matches!(
            s.insert(another_chain).await,
            Err(StoreError::InsertionFailed(
                StoreInsertionError::NeighborsVerificationFailed(_)
            ))
        ));
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_appends_with_gaps<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();
        gen.next_many(4);
        let header10 = gen.next();
        gen.next_many(4);
        let header15 = gen.next();

        s.insert(header5).await.unwrap();
        s.insert(header15).await.unwrap();
        s.insert(header10).await.unwrap_err();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_sampling_height_empty_store<S: Store>(
        #[case]
        #[future(awt)]
        store: S,
    ) {
        store
            .update_sampling_metadata(0, SamplingStatus::Accepted, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(1, SamplingStatus::Accepted, vec![])
            .await
            .unwrap_err();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_sampling_height<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut store = s;
        fill_store(&mut store, 9).await;

        store
            .update_sampling_metadata(0, SamplingStatus::Accepted, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(1, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(2, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(3, SamplingStatus::Rejected, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(4, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(5, SamplingStatus::Rejected, vec![])
            .await
            .unwrap();
        store
            .update_sampling_metadata(6, SamplingStatus::Rejected, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(8, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(7, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(9, SamplingStatus::Accepted, vec![])
            .await
            .unwrap();

        store
            .update_sampling_metadata(10, SamplingStatus::Accepted, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(10, SamplingStatus::Rejected, vec![])
            .await
            .unwrap_err();
        store
            .update_sampling_metadata(20, SamplingStatus::Accepted, vec![])
            .await
            .unwrap_err();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_sampling_merge<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut store = s;
        fill_store(&mut store, 1).await;

        let cid0 = "zdpuAyvkgEDQm9TenwGkd5eNaosSxjgEYd8QatfPetgB1CdEZ"
            .parse()
            .unwrap();
        let cid1 = "zb2rhe5P4gXftAwvA4eXQ5HJwsER2owDyS9sKaQRRVQPn93bA"
            .parse()
            .unwrap();
        let cid2 = "bafkreieq5jui4j25lacwomsqgjeswwl3y5zcdrresptwgmfylxo2depppq"
            .parse()
            .unwrap();

        store
            .update_sampling_metadata(1, SamplingStatus::Rejected, vec![cid0])
            .await
            .unwrap();

        store
            .update_sampling_metadata(1, SamplingStatus::Rejected, vec![])
            .await
            .unwrap();

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert_eq!(sampling_data.status, SamplingStatus::Rejected);
        assert_eq!(sampling_data.cids, vec![cid0]);

        store
            .update_sampling_metadata(1, SamplingStatus::Accepted, vec![cid1])
            .await
            .unwrap();

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert_eq!(sampling_data.status, SamplingStatus::Accepted);
        assert_eq!(sampling_data.cids, vec![cid0, cid1]);

        store
            .update_sampling_metadata(1, SamplingStatus::Accepted, vec![cid0, cid2])
            .await
            .unwrap();

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert_eq!(sampling_data.status, SamplingStatus::Accepted);
        assert_eq!(sampling_data.cids, vec![cid0, cid1, cid2]);
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_sampled_cids<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let mut store = s;
        fill_store(&mut store, 5).await;

        let cids: Vec<Cid> = [
            "bafkreieq5jui4j25lacwomsqgjeswwl3y5zcdrresptwgmfylxo2depppq",
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
            "zdpuAyvkgEDQm9TenwGkd5eNaosSxjgEYd8QatfPetgB1CdEZ",
            "zb2rhe5P4gXftAwvA4eXQ5HJwsER2owDyS9sKaQRRVQPn93bA",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();

        store
            .update_sampling_metadata(1, SamplingStatus::Accepted, cids.clone())
            .await
            .unwrap();
        store
            .update_sampling_metadata(2, SamplingStatus::Accepted, cids[0..1].to_vec())
            .await
            .unwrap();
        store
            .update_sampling_metadata(4, SamplingStatus::Rejected, cids[3..].to_vec())
            .await
            .unwrap();
        store
            .update_sampling_metadata(5, SamplingStatus::Rejected, vec![])
            .await
            .unwrap();

        let sampling_data = store.get_sampling_metadata(1).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids, cids);
        assert_eq!(sampling_data.status, SamplingStatus::Accepted);

        let sampling_data = store.get_sampling_metadata(2).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids, cids[0..1]);
        assert_eq!(sampling_data.status, SamplingStatus::Accepted);

        assert!(store.get_sampling_metadata(3).await.unwrap().is_none());

        let sampling_data = store.get_sampling_metadata(4).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids, cids[3..]);
        assert_eq!(sampling_data.status, SamplingStatus::Rejected);

        let sampling_data = store.get_sampling_metadata(5).await.unwrap().unwrap();
        assert_eq!(sampling_data.cids, vec![]);
        assert_eq!(sampling_data.status, SamplingStatus::Rejected);

        assert!(matches!(
            store.get_sampling_metadata(0).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.get_sampling_metadata(6).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.get_sampling_metadata(100).await,
            Err(StoreError::NotFound)
        ));
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_empty_store_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;

        assert_eq!(
            store.get_stored_header_ranges().await.unwrap().as_ref(),
            &[]
        );
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_single_header_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let mut gen = ExtendedHeaderGenerator::new();

        gen.skip(19);

        let prepend0 = gen.next();
        let prepend1 = gen.next_many_verified(5);
        store.insert(gen.next_many_verified(4)).await.unwrap();
        store.insert(gen.next_many_verified(5)).await.unwrap();
        store.insert(prepend1).await.unwrap();
        store.insert(prepend0).await.unwrap();
        store.insert(gen.next_many_verified(5)).await.unwrap();
        store.insert(gen.next()).await.unwrap();

        let final_ranges = store.get_stored_header_ranges().await.unwrap();
        assert_eq!(final_ranges.as_ref(), &[20..=40]);
    }

    // no in-memory store for tests below. It doesn't expect to be resumed from disk,
    // so it doesn't support multiple ranges.
    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_ranges_consolidation<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let mut gen = ExtendedHeaderGenerator::new();

        gen.skip(9);

        let skip0 = gen.next_many_verified(5);
        store.insert(gen.next_many_verified(2)).await.unwrap();
        store.insert(gen.next_many_verified(3)).await.unwrap();

        let skip1 = gen.next();
        store.insert(gen.next()).await.unwrap();

        let skip2 = gen.next_many_verified(5);

        store.insert(gen.next()).await.unwrap();

        let skip3 = gen.next_many_verified(5);
        let skip4 = gen.next_many_verified(5);
        let skip5 = gen.next_many_verified(5);

        store.insert(skip5).await.unwrap();
        store.insert(skip4).await.unwrap();
        store.insert(skip3).await.unwrap();
        store.insert(skip2).await.unwrap();
        store.insert(skip1).await.unwrap();
        store.insert(skip0).await.unwrap();

        let final_ranges = store.get_stored_header_ranges().await.unwrap();
        assert_eq!(final_ranges.as_ref(), &[10..=42]);
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn test_neighbour_validation<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let mut gen = ExtendedHeaderGenerator::new();

        store.insert(gen.next_many_verified(5)).await.unwrap();
        let mut fork = gen.fork();
        let _gap = gen.next();
        store.insert(gen.next_many_verified(4)).await.unwrap();

        store.insert(fork.next()).await.unwrap_err();
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn tail_removal_partial_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..64]).await.unwrap();
        store.insert(&headers[96..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 97..=128])).await;

        store.remove_height(1).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([2..=64, 97..=128])).await;
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn tail_removal_full_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..1]).await.unwrap();
        store.insert(&headers[65..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=1, 66..=128])).await;

        store.remove_height(1).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([66..=128])).await;
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn tail_removal_remove_all<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(66);

        store.insert(&headers[..]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=66])).await;

        for i in 1..=66 {
            store.remove_height(i).await.unwrap();
        }

        let stored_ranges = store.get_stored_header_ranges().await.unwrap();
        assert!(stored_ranges.is_empty());

        for h in 1..=66 {
            assert!(!store.has_at(h).await);
        }
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn head_removal_partial_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..64]).await.unwrap();
        store.insert(&headers[96..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 97..=128])).await;

        store.remove_height(128).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 97..=127])).await;
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn head_removal_full_range<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..64]).await.unwrap();
        store.insert(&headers[127..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 128..=128])).await;

        store.remove_height(128).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64])).await;
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn middle_removal<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..64]).await.unwrap();
        store.insert(&headers[96..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 97..=128])).await;

        store.remove_height(62).await.unwrap();
        assert_store(
            &store,
            &headers,
            new_block_ranges([1..=61, 63..=64, 97..=128]),
        )
        .await;

        store.remove_height(64).await.unwrap();
        assert_store(
            &store,
            &headers,
            new_block_ranges([1..=61, 63..=63, 97..=128]),
        )
        .await;

        store.remove_height(63).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=61, 97..=128])).await;
    }

    #[rstest]
    #[case::in_memory(new_in_memory_store())]
    #[cfg_attr(not(target_arch = "wasm32"), case::redb(new_redb_store()))]
    #[cfg_attr(target_arch = "wasm32", case::indexed_db(new_indexed_db_store()))]
    #[self::test]
    async fn neighbor_removal<S: Store>(
        #[case]
        #[future(awt)]
        s: S,
    ) {
        let store = s;
        let headers = ExtendedHeaderGenerator::new().next_many(128);

        store.insert(&headers[0..64]).await.unwrap();
        store.insert(&headers[96..128]).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=64, 97..=128])).await;

        store.remove_height(64).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=63, 97..=128])).await;

        store.remove_height(97).await.unwrap();
        assert_store(&store, &headers, new_block_ranges([1..=63, 98..=128])).await;
    }

    /// Fills an empty store
    async fn fill_store<S: Store>(store: &mut S, amount: u64) -> ExtendedHeaderGenerator {
        assert!(!store.has_at(1).await, "Store is not empty");

        let mut gen = ExtendedHeaderGenerator::new();

        store
            .insert(gen.next_many_verified(amount))
            .await
            .expect("inserting test data failed");

        gen
    }

    async fn new_in_memory_store() -> InMemoryStore {
        InMemoryStore::new()
    }

    pub(crate) async fn assert_store<S: Store>(
        store: &S,
        headers: &[ExtendedHeader],
        expected_ranges: BlockRanges,
    ) {
        assert_eq!(
            store.get_stored_header_ranges().await.unwrap(),
            expected_ranges
        );
        for header in headers {
            let height = header.height().value();
            if expected_ranges.contains(height) {
                assert_eq!(&store.get_by_height(height).await.unwrap(), header);
                assert_eq!(&store.get_by_hash(&header.hash()).await.unwrap(), header);
            } else {
                assert!(matches!(
                    store.get_by_height(height).await.unwrap_err(),
                    StoreError::NotFound
                ));
                assert!(matches!(
                    store.get_by_hash(&header.hash()).await.unwrap_err(),
                    StoreError::NotFound
                ));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn new_redb_store() -> RedbStore {
        RedbStore::in_memory().await.unwrap()
    }

    #[cfg(target_arch = "wasm32")]
    async fn new_indexed_db_store() -> IndexedDbStore {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let db_name = format!("indexeddb-lumina-node-store-test-{id}");

        // DB can persist if test run within the browser
        rexie::Rexie::delete(&db_name).await.unwrap();

        IndexedDbStore::new(&db_name)
            .await
            .expect("creating test store failed")
    }
}
