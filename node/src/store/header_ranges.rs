use std::fmt::Display;
use std::ops::RangeInclusive;

use serde::Serialize;
use smallvec::{IntoIter, SmallVec};
use thiserror::Error;

use crate::store::utils::{ranges_intersection, try_consolidate_ranges, RangeScanResult};
use crate::store::StoreError;

pub type HeaderRange = RangeInclusive<u64>;

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

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct HeaderRanges(SmallVec<[HeaderRange; 2]>);

#[derive(Debug, Error)]
pub enum HeaderRangeError {
    #[error("Overlapping range: {0}, {1}")]
    RangeOverlap(u64, u64),
}

pub(crate) trait HeaderRangesExt {
    /// Check that sub-ranges do not overlap
    fn validate(&self) -> Result<(), HeaderRangeError>;
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
    fn validate(&self) -> Result<(), HeaderRangeError> {
        let mut prev: Option<&HeaderRange> = None;
        for current in &self.0 {
            if let Some(prev) = prev {
                if current.start() > prev.end() {
                    return Err(HeaderRangeError::RangeOverlap(
                        *current.start(),
                        *prev.end(),
                    ));
                }
            }
            prev = Some(current);
        }
        Ok(())
    }

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

impl AsRef<[RangeInclusive<u64>]> for HeaderRanges {
    fn as_ref(&self) -> &[RangeInclusive<u64>] {
        &self.0
    }
}

impl FromIterator<RangeInclusive<u64>> for HeaderRanges {
    fn from_iter<T: IntoIterator<Item = RangeInclusive<u64>>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<const T: usize> From<[RangeInclusive<u64>; T]> for HeaderRanges {
    fn from(value: [RangeInclusive<u64>; T]) -> Self {
        Self(value.into_iter().collect())
    }
}

impl IntoIterator for HeaderRanges {
    type Item = u64;
    type IntoIter = HeaderRangesIterator;
    fn into_iter(self) -> Self::IntoIter {
        let mut outer_iter = self.0.into_iter();
        HeaderRangesIterator {
            inner_iter: outer_iter.next(),
            outer_iter,
        }
    }
}

pub struct HeaderRangesIterator {
    inner_iter: Option<HeaderRange>,
    outer_iter: IntoIter<[HeaderRange; 2]>,
}

impl HeaderRangesIterator {
    pub fn next_batch(&mut self, limit: u64) -> Option<HeaderRange> {
        let current_range = self.inner_iter.take()?;

        if current_range.len() <= limit {
            self.inner_iter = self.outer_iter.next();
            Some(current_range)
        } else {
            let returned_range = *current_range.start()..=*current_range.start() + limit - 1;
            self.inner_iter = Some(*current_range.start() + limit..=*current_range.end());
            Some(returned_range)
        }
    }
}

impl Iterator for HeaderRangesIterator {
    type Item = u64;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(v) = self.inner_iter.as_mut()?.next() {
                return Some(v);
            }
            self.inner_iter = self.outer_iter.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_len() {
        assert_eq!((0u64..=0).len(), 1);
        assert_eq!((0u64..=5).len(), 6);
        assert_eq!((1u64..=2).len(), 2);
        assert_eq!((2u64..=1).len(), 0);
        assert_eq!((10001u64..=20000).len(), 10000);
    }

    #[test]
    fn test_iter() {
        let ranges = HeaderRanges::from([1..=5, 7..=10]);
        assert_eq!(
            ranges.into_iter().collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 7, 8, 9, 10]
        );

        let ranges = HeaderRanges::from([1..=1, 2..=4, 8..=8]);
        assert_eq!(ranges.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 8]);

        let mut iter = HeaderRanges::from([1..=1]).into_iter();
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_iter_batches() {
        let mut ranges = HeaderRanges::from([1..=100]).into_iter();
        assert_eq!(ranges.next_batch(10), Some(1..=10));
        assert_eq!(ranges.next_batch(10), Some(11..=20));
        assert_eq!(ranges.next_batch(100), Some(21..=100));

        let mut ranges = HeaderRanges::from([1..=10, 21..=30, 41..=50]).into_iter();
        assert_eq!(ranges.next_batch(20), Some(1..=10));
        assert_eq!(ranges.next_batch(1), Some(21..=21));
        assert_eq!(ranges.next_batch(2), Some(22..=23));
        assert_eq!(ranges.next_batch(3), Some(24..=26));
        assert_eq!(ranges.next_batch(4), Some(27..=30));
        assert_eq!(ranges.next_batch(5), Some(41..=45));
        assert_eq!(ranges.next_batch(100), Some(46..=50));
    }

    #[test]
    fn header_ranges_empty() {
        assert!(HeaderRanges::from([]).is_empty());
        assert!(!HeaderRanges::from([1..=3]).is_empty());
    }

    #[test]
    fn header_ranges_head() {
        assert_eq!(HeaderRanges::from([]).head(), None);
        assert_eq!(HeaderRanges::from([1..=3]).head(), Some(3));
        assert_eq!(HeaderRanges::from([1..=3, 6..=9]).head(), Some(9));
        assert_eq!(HeaderRanges::from([1..=3, 5..=5, 8..=9]).head(), Some(9));
    }

    #[test]
    fn header_ranges_tail() {
        assert_eq!(HeaderRanges::from([]).tail(), None);
        assert_eq!(HeaderRanges::from([1..=3]).tail(), Some(1));
        assert_eq!(HeaderRanges::from([1..=3, 6..=9]).tail(), Some(1));
        assert_eq!(HeaderRanges::from([1..=3, 5..=5, 8..=9]).tail(), Some(1));
    }

    #[test]
    fn check_range_insert_append() {
        let result = HeaderRanges::from([]).check_range_insert(&(1..=5)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = HeaderRanges::from([1..=4])
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

        let result = HeaderRanges::from([1..=5])
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

        let result = HeaderRanges::from([6..=8])
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
        let result = HeaderRanges::from([1..=3, 6..=9])
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

        let result = HeaderRanges::from([1..=2, 5..=5, 8..=9])
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

        let result = HeaderRanges::from([1..=2, 4..=4, 8..=9])
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
        let result = HeaderRanges::from([1..=2])
            .check_range_insert(&(1..=1))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(1, 1)));

        let result = HeaderRanges::from([1..=4])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 4)));

        let result = HeaderRanges::from([1..=4])
            .check_range_insert(&(2..=3))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 3)));

        let result = HeaderRanges::from([5..=9])
            .check_range_insert(&(1..=5))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 5)));

        let result = HeaderRanges::from([5..=8])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 8)));

        let result = HeaderRanges::from([1..=3, 6..=9])
            .check_range_insert(&(3..=6))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(3, 3)));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = HeaderRanges::from([1..=2, 7..=9])
            .check_range_insert(&(4..=4))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = HeaderRanges::from([1..=2, 8..=9])
            .check_range_insert(&(4..=6))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = HeaderRanges::from([4..=5, 7..=8])
            .check_range_insert(&(1..=2))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));
    }
}
