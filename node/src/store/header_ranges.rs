use std::fmt::Display;
use std::ops::RangeInclusive;
use std::vec;

#[cfg(any(test, feature = "test-utils"))]
use celestia_types::test_utils::ExtendedHeaderGenerator;
use celestia_types::ExtendedHeader;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::store::utils::{ranges_intersection, try_consolidate_ranges, RangeScanResult};
use crate::store::StoreError;

pub type HeaderRange = RangeInclusive<u64>;

/// Span of header that's been verified internally
#[derive(Clone)]
pub struct VerifiedExtendedHeaders(Vec<ExtendedHeader>);

impl IntoIterator for VerifiedExtendedHeaders {
    type Item = ExtendedHeader;
    type IntoIter = vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> TryFrom<&'a [ExtendedHeader]> for VerifiedExtendedHeaders {
    type Error = celestia_types::Error;

    fn try_from(value: &'a [ExtendedHeader]) -> Result<Self, Self::Error> {
        value.to_vec().try_into()
    }
}

impl From<VerifiedExtendedHeaders> for Vec<ExtendedHeader> {
    fn from(value: VerifiedExtendedHeaders) -> Self {
        value.0
    }
}

impl AsRef<[ExtendedHeader]> for VerifiedExtendedHeaders {
    fn as_ref(&self) -> &[ExtendedHeader] {
        &self.0
    }
}

/// 1-length hedaer span is internally verified, this is valid
impl From<[ExtendedHeader; 1]> for VerifiedExtendedHeaders {
    fn from(value: [ExtendedHeader; 1]) -> Self {
        Self(value.into())
    }
}

impl From<ExtendedHeader> for VerifiedExtendedHeaders {
    fn from(value: ExtendedHeader) -> Self {
        Self(vec![value])
    }
}

impl<'a> From<&'a ExtendedHeader> for VerifiedExtendedHeaders {
    fn from(value: &ExtendedHeader) -> Self {
        Self(vec![value.to_owned()])
    }
}

impl TryFrom<Vec<ExtendedHeader>> for VerifiedExtendedHeaders {
    type Error = celestia_types::Error;

    fn try_from(headers: Vec<ExtendedHeader>) -> Result<Self, Self::Error> {
        let Some(head) = headers.first() else {
            return Ok(VerifiedExtendedHeaders(Vec::default()));
        };

        head.verify_adjacent_range(&headers[1..])?;

        Ok(Self(headers))
    }
}

impl VerifiedExtendedHeaders {
    /// Create a new instance out of pre-checked vec of headers
    ///
    /// # Safety
    ///
    /// This function may produce invalid `VerifiedExtendedHeaders`, if passed range is not
    /// validated manually
    pub unsafe fn new_unchecked(headers: Vec<ExtendedHeader>) -> Self {
        Self(headers)
    }
}

pub(crate) trait RangeLengthExt {
    fn len(&self) -> u64;
}

impl RangeLengthExt for RangeInclusive<u64> {
    fn len(&self) -> u64 {
        match self.end().checked_sub(*self.start()) {
            Some(difference) => difference + 1,
            None => 0,
        }
    }
}

/// Represents possibly multiple non-overlapping, sorted ranges of header heights
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HeaderRanges(SmallVec<[HeaderRange; 2]>);

pub(crate) trait HeaderRangesExt {
    /// Check whether provided `to_insert` range can be inserted into the header ranges represented
    /// by self. New range can be inserted ahead of all existing ranges to allow syncing from the
    /// head but otherwise, only growing the existing ranges is allowed.
    /// Returned [`RangeScanResult`] contains information necessary to persist the range
    /// modification in the database manually, or one can call [`update_range`] to modify ranges in
    /// memory.
    fn check_range_insert(&self, to_insert: &HeaderRange) -> Result<RangeScanResult, StoreError>;
    /// Modify the header ranges, committing insert previously checked with [`check_range_insert`]
    fn update_range(&mut self, scan_information: RangeScanResult);
}

impl HeaderRangesExt for HeaderRanges {
    fn update_range(&mut self, scan_information: RangeScanResult) {
        let RangeScanResult {
            range_index,
            range,
            range_to_remove,
        } = scan_information;

        if self.0.len() == range_index {
            self.0.push(range);
        } else {
            self.0[range_index] = range;
        }

        if let Some(to_remove) = range_to_remove {
            self.0.remove(to_remove);
        }
    }

    fn check_range_insert(&self, to_insert: &HeaderRange) -> Result<RangeScanResult, StoreError> {
        let Some(head_range) = self.0.last() else {
            // Empty store case
            return Ok(RangeScanResult {
                range_index: 0,
                range: to_insert.clone(),
                range_to_remove: None,
            });
        };

        // allow inserting a new header range in front of the current head range
        // +1 in here to let ranges merge below in case they're contiguous
        if *to_insert.start() > head_range.end() + 1 {
            return Ok(RangeScanResult {
                range_index: self.0.len(),
                range: to_insert.clone(),
                range_to_remove: None,
            });
        }

        let mut stored_ranges_iter = self.0.iter().enumerate();
        let mut found_range = loop {
            let Some((idx, stored_range)) = stored_ranges_iter.next() else {
                return Err(StoreError::InsertPlacementDisallowed(
                    *to_insert.start(),
                    *to_insert.end(),
                ));
            };

            if let Some(intersection) = ranges_intersection(stored_range, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(stored_range, to_insert) {
                break RangeScanResult {
                    range_index: idx,
                    range: consolidated,
                    range_to_remove: None,
                };
            }
        };

        // we have a hit, check whether we can merge with the next range too
        if let Some((idx, range_after)) = stored_ranges_iter.next() {
            if let Some(intersection) = ranges_intersection(range_after, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(range_after, &found_range.range) {
                found_range = RangeScanResult {
                    range_index: found_range.range_index,
                    range: consolidated,
                    range_to_remove: Some(idx),
                };
            }
        }

        Ok(found_range)
    }
}

impl HeaderRanges {
    /// Return whether `HeaderRanges` contains provided height
    pub fn contains(&self, height: u64) -> bool {
        self.0.iter().any(|r| r.contains(&height))
    }

    /// Return whether range is empty
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|r| r.is_empty())
    }

    /// Return highest height in the range
    pub fn head(&self) -> Option<u64> {
        self.0.last().map(|r| *r.end())
    }

    /// Return lowest height in the range
    pub fn tail(&self) -> Option<u64> {
        self.0.first().map(|r| *r.start())
    }

    /// Crate HeaderRanges from correctly pre-sorted, non-overlapping SmallVec of ranges
    pub(crate) fn from_vec(from: SmallVec<[HeaderRange; 2]>) -> Self {
        #[cfg(debug_assertions)]
        {
            let mut prev: Option<&RangeInclusive<u64>> = None;

            for range in &from {
                assert!(
                    range.start() <= range.end(),
                    "range isn't sorted internally"
                );

                if let Some(prev) = prev {
                    assert!(
                        prev.end() < range.start(),
                        "header ranges aren't sorted correctly"
                    );
                }

                prev = Some(range);
            }
        }

        Self(from)
    }
}

impl Display for HeaderRanges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for (idx, range) in self.0.iter().enumerate() {
            if idx == 0 {
                write!(f, "{}-{}", range.start(), range.end())?;
            } else {
                write!(f, ", {}-{}", range.start(), range.end())?;
            }
        }
        write!(f, "]")
    }
}

pub(crate) struct PrintableHeaderRange(pub RangeInclusive<u64>);

impl Display for PrintableHeaderRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.0.start(), self.0.end())
    }
}

impl AsRef<[RangeInclusive<u64>]> for HeaderRanges {
    fn as_ref(&self) -> &[RangeInclusive<u64>] {
        &self.0
    }
}

/// Extends test header generator for easier insertion into the store
pub trait ExtendedHeaderGeneratorExt {
    /// Generate next amount verified headers
    fn next_many_verified(&mut self, amount: u64) -> VerifiedExtendedHeaders;
}

#[cfg(any(test, feature = "test-utils"))]
impl ExtendedHeaderGeneratorExt for ExtendedHeaderGenerator {
    fn next_many_verified(&mut self, amount: u64) -> VerifiedExtendedHeaders {
        unsafe { VerifiedExtendedHeaders::new_unchecked(self.next_many(amount)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn range_len() {
        assert_eq!((0u64..=0).len(), 1);
        assert_eq!((0u64..=5).len(), 6);
        assert_eq!((1u64..=2).len(), 2);
        assert_eq!(RangeInclusive::new(2u64, 1).len(), 0);
        assert_eq!((10001u64..=20000).len(), 10000);
    }

    #[test]
    fn header_ranges_empty() {
        assert!(HeaderRanges::from_vec(smallvec![]).is_empty());
        assert!(!HeaderRanges::from_vec(smallvec![1..=3]).is_empty());
    }

    #[test]
    fn header_ranges_head() {
        assert_eq!(HeaderRanges::from_vec(smallvec![]).head(), None);
        assert_eq!(HeaderRanges::from_vec(smallvec![1..=3]).head(), Some(3));
        assert_eq!(
            HeaderRanges::from_vec(smallvec![1..=3, 6..=9]).head(),
            Some(9)
        );
        assert_eq!(
            HeaderRanges::from_vec(smallvec![1..=3, 5..=5, 8..=9]).head(),
            Some(9)
        );
    }

    #[test]
    fn header_ranges_tail() {
        assert_eq!(HeaderRanges::from_vec(smallvec![]).tail(), None);
        assert_eq!(HeaderRanges::from_vec(smallvec![1..=3]).tail(), Some(1));
        assert_eq!(
            HeaderRanges::from_vec(smallvec![1..=3, 6..=9]).tail(),
            Some(1)
        );
        assert_eq!(
            HeaderRanges::from_vec(smallvec![1..=3, 5..=5, 8..=9]).tail(),
            Some(1)
        );
    }

    #[test]
    fn check_range_insert_append() {
        let result = HeaderRanges::from_vec(smallvec![])
            .check_range_insert(&(1..=5))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = HeaderRanges::from_vec(smallvec![1..=4])
            .check_range_insert(&(5..=5))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = HeaderRanges::from_vec(smallvec![1..=5])
            .check_range_insert(&(6..=9))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: None,
            }
        );

        let result = HeaderRanges::from_vec(smallvec![6..=8])
            .check_range_insert(&(2..=5))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 2..=8,
                range_to_remove: None,
            }
        );
    }

    #[test]
    fn check_range_insert_with_consolidation() {
        let result = HeaderRanges::from_vec(smallvec![1..=3, 6..=9])
            .check_range_insert(&(4..=5))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: Some(1),
            }
        );

        let result = HeaderRanges::from_vec(smallvec![1..=2, 5..=5, 8..=9])
            .check_range_insert(&(3..=4))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: Some(1),
            }
        );

        let result = HeaderRanges::from_vec(smallvec![1..=2, 4..=4, 8..=9])
            .check_range_insert(&(5..=7))
            .unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 1,
                range: 4..=9,
                range_to_remove: Some(2),
            }
        );
    }

    #[test]
    fn check_range_insert_overlapping() {
        let result = HeaderRanges::from_vec(smallvec![1..=2])
            .check_range_insert(&(1..=1))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(1, 1)));

        let result = HeaderRanges::from_vec(smallvec![1..=4])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 4)));

        let result = HeaderRanges::from_vec(smallvec![1..=4])
            .check_range_insert(&(2..=3))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 3)));

        let result = HeaderRanges::from_vec(smallvec![5..=9])
            .check_range_insert(&(1..=5))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 5)));

        let result = HeaderRanges::from_vec(smallvec![5..=8])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 8)));

        let result = HeaderRanges::from_vec(smallvec![1..=3, 6..=9])
            .check_range_insert(&(3..=6))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(3, 3)));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = HeaderRanges::from_vec(smallvec![1..=2, 7..=9])
            .check_range_insert(&(4..=4))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = HeaderRanges::from_vec(smallvec![1..=2, 8..=9])
            .check_range_insert(&(4..=6))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = HeaderRanges::from_vec(smallvec![4..=5, 7..=8])
            .check_range_insert(&(1..=2))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));
    }

    #[test]
    fn test_header_range_creation_ok() {
        HeaderRanges::from_vec(smallvec![1..=3, 5..=8]);
        HeaderRanges::from_vec(smallvec![]);
        HeaderRanges::from_vec(smallvec![1..=1, 1000000..=2000000]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_overlap() {
        HeaderRanges::from_vec(smallvec![1..=3, 2..=5]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_inverse() {
        HeaderRanges::from_vec(smallvec![1..=3, RangeInclusive::new(9, 5)]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_wrong_order() {
        HeaderRanges::from_vec(smallvec![10..=15, 1..=5]);
    }
}
