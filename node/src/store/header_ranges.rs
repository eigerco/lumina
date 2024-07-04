use std::borrow::Borrow;
use std::fmt::{Debug, Display};
use std::ops::{RangeInclusive, Sub};

use serde::Serialize;
use smallvec::SmallVec;

use crate::store::utils::{ranges_intersection, try_consolidate_ranges, RangeScanResult};
use crate::store::StoreError;

pub type BlockRange = RangeInclusive<u64>;

#[derive(Debug, thiserror::Error)]
pub enum BlockRangesError {
    #[error("Invalid block range: {}-{}", .0.start(), .0.end())]
    InvalidBlockRange(BlockRange),

    #[error("Block ranges are not sorted")]
    UnsortedBlockRanges,

    #[error("Insersion disallowed")]
    InsertDisallowed(BlockRange),
}

type Result<T, E = BlockRangesError> = std::result::Result<T, E>;

pub trait BlockRangeExt {
    fn validate(&self) -> Result<()>;
    fn len(&self) -> u64;
    fn is_adjacent(&self, other: &BlockRange) -> bool;
    fn is_overlapping(&self, other: &BlockRange) -> bool;
}

impl BlockRangeExt for BlockRange {
    fn validate(&self) -> Result<()> {
        if *self.start() > 0 && self.start() <= self.end() {
            Ok(())
        } else {
            Err(BlockRangesError::InvalidBlockRange(self.to_owned()))
        }
    }

    fn len(&self) -> u64 {
        match self.end().checked_sub(*self.start()) {
            Some(difference) => difference + 1,
            None => 0,
        }
    }

    fn is_adjacent(&self, other: &BlockRange) -> bool {
        debug_assert!(self.validate().is_ok());
        debug_assert!(other.validate().is_ok());

        // End of `self` touches start of `other`
        //
        // self:  |------|
        // other:         |------|
        if *self.end() == other.start().saturating_sub(1) {
            return true;
        }

        // Start of `self` touches end of `other`
        //
        // self:          |------|
        // other: |------|
        if self.start().saturating_sub(1) == *other.end() {
            return true;
        }

        false
    }

    fn is_overlapping(&self, other: &BlockRange) -> bool {
        debug_assert!(self.validate().is_ok());
        debug_assert!(other.validate().is_ok());

        // `self` is partial set of `other`, case 1
        //
        // self:  |------|
        // other:     |------|
        if self.start() < other.start() && other.contains(self.end()) {
            return true;
        }

        // `self` is partial set of `other`, case 2
        //
        // self:      |------|
        // other: |------|
        if self.end() > other.end() && other.contains(self.start()) {
            return true;
        }

        // `self` is subset of `other`
        //
        // self:    |--|
        // other: |------|
        if self.start() >= other.start() && self.end() <= other.end() {
            return true;
        }

        // `self` is superset of `other`
        //
        // self:  |------|
        // other:   |--|
        if self.start() <= other.start() && self.end() >= other.end() {
            return true;
        }

        false
    }
}

/// Represents possibly multiple non-overlapping, sorted ranges of header heights
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(transparent)]
pub struct BlockRanges(SmallVec<[BlockRange; 2]>);

/// Custom `Deserialize` that validates `BlockRanges`.
impl<'de> serde::Deserialize<'de> for BlockRanges {
    fn deserialize<D>(deserializer: D) -> Result<BlockRanges, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let raw_ranges = SmallVec::<[RangeInclusive<u64>; 2]>::deserialize(deserializer)?;
        let ranges = BlockRanges(raw_ranges);

        ranges
            .validate()
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        Ok(ranges)
    }
}

pub(crate) trait BlockRangesExt {
    /// Check whether provided `to_insert` range can be inserted into the header ranges represented
    /// by self. New range can be inserted ahead of all existing ranges to allow syncing from the
    /// head but otherwise, only growing the existing ranges is allowed.
    /// Returned [`RangeScanResult`] contains information necessary to persist the range
    /// modification in the database manually, or one can call [`update_range`] to modify ranges in
    /// memory.
    fn check_range_insert(&self, to_insert: &BlockRange) -> Result<RangeScanResult, StoreError>;
    /// Modify the header ranges, committing insert previously checked with [`check_range_insert`]
    fn update_range(&mut self, scan_information: RangeScanResult);
}

impl BlockRangesExt for BlockRanges {
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

    fn check_range_insert(&self, to_insert: &BlockRange) -> Result<RangeScanResult, StoreError> {
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

            if let Some(intersection) = ranges_intersection(&stored_range, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(&stored_range, to_insert) {
                break RangeScanResult {
                    range_index: idx,
                    range: consolidated,
                    range_to_remove: None,
                };
            }
        };

        // we have a hit, check whether we can merge with the next range too
        if let Some((idx, range_after)) = stored_ranges_iter.next() {
            if let Some(intersection) = ranges_intersection(&range_after, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(&range_after, &found_range.range) {
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

#[derive(Debug, Clone, Copy)]
enum Strategy {
    /// Finds overlapping ranges.
    Overlapping,
    /// Finds adjacent ranges.
    Adjacent,
    /// Finds overlapping and adjacent ranges.
    Intersecting,
}

impl BlockRanges {
    pub(crate) fn from_vec(from: SmallVec<[BlockRange; 2]>) -> Result<Self> {
        let ranges = BlockRanges(from);
        ranges.validate()?;
        Ok(ranges)
    }

    /// Returns `Ok()` if `BlockRanges` hold valid and sorted ranges
    pub(crate) fn validate(&self) -> Result<()> {
        let mut prev: Option<&RangeInclusive<u64>> = None;

        for range in self.0.iter() {
            range.validate()?;

            if let Some(prev) = prev {
                if range.start() <= prev.end() {
                    return Err(BlockRangesError::UnsortedBlockRanges);
                }
            }

            prev = Some(range);
        }

        Ok(())
    }

    /// Return whether `BlockRanges` contains provided height
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

    /// Return `Ok(left_neighbor, right_neighbor)` if `height` passes the header store contrains.
    pub fn check_header_store_constrains(
        &self,
        to_insert: BlockRange,
    ) -> Result<(Option<u64>, Option<u64>)> {
        to_insert.validate()?;

        if self.is_empty() {
            // Allow insersion on empty store.
            return Ok((None, None));
        }

        // Get the indexes of the afffected ranges.
        let Some((start_idx, end_idx)) = self.find_ranges(&to_insert, Strategy::Adjacent) else {
            return Err(BlockRangesError::InsertDisallowed(to_insert));
        };

        debug_assert!(end_idx - start_idx <= 1);

        // If only one range is affected.
        if start_idx == end_idx {
            let range = &self.0[start_idx];

            // If the `range` is on the left side of `to_insert`.
            if range.end() < to_insert.start() {
                Ok((Some(*range.end()), None))
            } else {
                // `range` is on the right side of `to_insert`.
                Ok((None, Some(*range.start())))
            }
        } else {
            let left_range = &self.0[start_idx];
            let right_range = &self.0[end_idx];

            Ok((Some(*left_range.end()), Some(*right_range.start())))
        }
    }

    /// Returns the start index and end index of the affected ranges.
    fn find_ranges(
        &self,
        range: impl Borrow<BlockRange>,
        strategy: Strategy,
    ) -> Option<(usize, usize)> {
        let range = range.borrow();
        debug_assert!(range.validate().is_ok());

        let mut start_idx = None;
        let mut end_idx = None;

        for (i, r) in self.0.iter().enumerate() {
            let found = match strategy {
                Strategy::Overlapping => r.is_overlapping(range),
                Strategy::Adjacent => r.is_adjacent(range),
                Strategy::Intersecting => r.is_overlapping(range) || r.is_adjacent(range),
            };

            if found {
                if start_idx.is_none() {
                    start_idx = Some(i);
                }

                end_idx = Some(i);
            } else if end_idx.is_some() {
                // We reached from a satisfying range to a non-satisfying.
                // That means there nothing else to find.
                break;
            }
        }

        Some((start_idx?, end_idx?))
    }

    /// Returns an inverted `BlockRanges` from 1 up to HEAD.
    pub(crate) fn inverted_up_to_head(&self) -> BlockRanges {
        let mut next_start = 1;
        let mut ranges = SmallVec::new();

        for range in self.0.iter() {
            if next_start < *range.start() {
                ranges.push(next_start..=*range.start() - 1);
            }
            next_start = *range.end() + 1;
        }

        BlockRanges(ranges)
    }

    pub(crate) fn pop_head(&mut self) -> Option<u64> {
        let last = self.0.last_mut()?;
        let head = *last.end();

        if last.len() == 1 {
            self.0.remove(self.0.len() - 1);
        } else {
            *last = *last.start()..=*last.end() - 1;
        }

        Some(head)
    }

    pub(crate) fn insert_relaxed(&mut self, range: BlockRange) -> Result<()> {
        range.validate()?;

        match self.find_ranges(&range, Strategy::Intersecting) {
            // `range` must be merged with other ranges
            Some((start_idx, end_idx)) => {
                let start = *self.0[start_idx].start().min(range.start());
                let end = *self.0[end_idx].end().max(range.end());

                self.0.drain(start_idx..=end_idx);
                self.0.insert(start_idx, start..=end);
            }
            // `range` can not be merged with other ranges
            None => {
                for (i, r) in self.0.iter().enumerate() {
                    if range.end() < r.start() {
                        self.0.insert(i, range);
                        return Ok(());
                    }
                }

                self.0.push(range);
            }
        }

        Ok(())
    }

    pub(crate) fn remove_relaxed(&mut self, range: BlockRange) -> Result<()> {
        range.validate()?;

        let Some((start_idx, end_idx)) = self.find_ranges(&range, Strategy::Overlapping) else {
            // Nothing to remove
            return Ok(());
        };

        // Remove old ranges
        let old_ranges = self
            .0
            .drain(start_idx..=end_idx)
            .collect::<SmallVec<[_; 2]>>();

        // ranges:       |-----|  |----|  |----|
        // remove:           |--------------|
        // after remove: |---|              |--|
        let first_range = old_ranges.first().expect("non empty");
        let last_range = old_ranges.last().expect("non empty");

        if range.end() < last_range.end() {
            // Add the right range
            self.0
                .insert(start_idx, *range.end() + 1..=*last_range.end());
        }

        if first_range.start() < range.start() {
            // Add the left range
            self.0
                .insert(start_idx, *first_range.start()..=*range.start() - 1);
        }

        Ok(())
    }
}

impl AsRef<[RangeInclusive<u64>]> for BlockRanges {
    fn as_ref(&self) -> &[RangeInclusive<u64>] {
        &self.0
    }
}

impl Sub for BlockRanges {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.sub(&rhs)
    }
}

impl Sub<&BlockRanges> for BlockRanges {
    type Output = Self;

    fn sub(mut self, rhs: &BlockRanges) -> Self::Output {
        for range in rhs.0.iter() {
            self.remove_relaxed(range.to_owned())
                .expect("BlockRanges always holds valid ranges");
        }
        self
    }
}

impl Display for BlockRanges {
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

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    fn new_block_ranges<const N: usize>(ranges: [BlockRange; N]) -> BlockRanges {
        BlockRanges::from_vec(ranges.into_iter().collect()).expect("invalid BlockRanges")
    }

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
        assert!(new_block_ranges([]).is_empty());
        assert!(!new_block_ranges([1..=3]).is_empty());
    }

    #[test]
    fn header_ranges_head() {
        assert_eq!(new_block_ranges([]).head(), None);
        assert_eq!(new_block_ranges([1..=3]).head(), Some(3));
        assert_eq!(new_block_ranges([1..=3, 6..=9]).head(), Some(9));
        assert_eq!(new_block_ranges([1..=3, 5..=5, 8..=9]).head(), Some(9));
    }

    #[test]
    fn header_ranges_tail() {
        assert_eq!(new_block_ranges([]).tail(), None);
        assert_eq!(new_block_ranges([1..=3]).tail(), Some(1));
        assert_eq!(new_block_ranges([1..=3, 6..=9]).tail(), Some(1));
        assert_eq!(new_block_ranges([1..=3, 5..=5, 8..=9]).tail(), Some(1));
    }

    #[test]
    fn check_range_insert_append() {
        let (left, right) = new_block_ranges([])
            .check_header_store_constrains(1..=5)
            .unwrap();
        assert_eq!(left, None);
        assert_eq!(right, None);

        let (left, right) = new_block_ranges([1..=4])
            .check_header_store_constrains(5..=5)
            .unwrap();
        assert_eq!(left, Some(4));
        assert_eq!(right, None);

        let (left, right) = new_block_ranges([1..=5])
            .check_header_store_constrains(6..=9)
            .unwrap();
        assert_eq!(left, Some(5));
        assert_eq!(right, None);

        let (left, right) = new_block_ranges([6..=8])
            .check_header_store_constrains(2..=5)
            .unwrap();
        assert_eq!(left, None);
        assert_eq!(right, Some(6));
    }

    #[test]
    fn check_range_insert_with_consolidation() {
        let (left, right) = new_block_ranges([1..=3, 6..=9])
            .check_header_store_constrains(4..=5)
            .unwrap();
        assert_eq!(left, Some(3));
        assert_eq!(right, Some(6));

        let (left, right) = new_block_ranges([1..=2, 5..=5, 8..=9])
            .check_header_store_constrains(3..=4)
            .unwrap();
        assert_eq!(left, Some(2));
        assert_eq!(right, Some(5));

        let (left, right) = new_block_ranges([1..=2, 4..=4, 8..=9])
            .check_header_store_constrains(5..=7)
            .unwrap();
        assert_eq!(left, Some(4));
        assert_eq!(right, Some(8));
    }

    #[test]
    fn check_range_insert_overlapping() {
        let result = new_block_ranges([1..=2])
            .check_range_insert(&(1..=1))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(1, 1)));

        let result = new_block_ranges([1..=4])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 4)));

        let result = new_block_ranges([1..=4])
            .check_range_insert(&(2..=3))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 3)));

        let result = new_block_ranges([5..=9])
            .check_range_insert(&(1..=5))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 5)));

        let result = new_block_ranges([5..=8])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 8)));

        let result = new_block_ranges([1..=3, 6..=9])
            .check_range_insert(&(3..=6))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(3, 3)));

        let result = new_block_ranges([1..=2])
            .check_header_store_constrains(1..=1)
            .unwrap_err();

        let result = new_block_ranges([1..=4])
            .check_header_store_constrains(2..=8)
            .unwrap_err();

        let result = new_block_ranges([1..=4])
            .check_header_store_constrains(2..=3)
            .unwrap_err();

        let result = new_block_ranges([5..=9])
            .check_header_store_constrains(1..=5)
            .unwrap_err();

        let result = new_block_ranges([5..=8])
            .check_header_store_constrains(2..=8)
            .unwrap_err();

        let result = new_block_ranges([1..=3, 6..=9])
            .check_header_store_constrains(3..=6)
            .unwrap_err();
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = new_block_ranges([1..=2, 7..=9])
            .check_range_insert(&(4..=4))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = new_block_ranges([1..=2, 8..=9])
            .check_range_insert(&(4..=6))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = new_block_ranges([4..=5, 7..=8])
            .check_range_insert(&(1..=2))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));

        let result = new_block_ranges([1..=2, 7..=9])
            .check_header_store_constrains(4..=4)
            .unwrap_err();

        let result = new_block_ranges([1..=2, 8..=9])
            .check_header_store_constrains(4..=6)
            .unwrap_err();

        let result = new_block_ranges([4..=5, 7..=8])
            .check_header_store_constrains(1..=2)
            .unwrap_err();
    }

    #[test]
    fn test_header_range_creation_ok() {
        new_block_ranges([1..=3, 5..=8]);
        new_block_ranges([]);
        new_block_ranges([1..=1, 1000000..=2000000]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_overlap() {
        new_block_ranges([1..=3, 2..=5]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_inverse() {
        new_block_ranges([1..=3, RangeInclusive::new(9, 5)]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_wrong_order() {
        new_block_ranges([10..=15, 1..=5]);
    }

    #[test]
    fn inverted() {
        assert_eq!(
            BlockRanges::default().inverted_up_to_head(),
            BlockRanges::default()
        );
        assert_eq!(
            new_block_ranges([1..=100]).inverted_up_to_head(),
            BlockRanges::default()
        );
        assert_eq!(
            new_block_ranges([2..=100]).inverted_up_to_head(),
            new_block_ranges([1..=1])
        );
        assert_eq!(
            new_block_ranges([2..=100, 102..=102]).inverted_up_to_head(),
            new_block_ranges([1..=1, 101..=101])
        );

        assert_eq!(
            new_block_ranges([2..=100, 150..=160, 200..=210, 300..=310]).inverted_up_to_head(),
            new_block_ranges([1..=1, 101..=149, 161..=199, 211..=299])
        );
    }

    #[test]
    fn pop_head() {
        let mut ranges = new_block_ranges([]);
        assert_eq!(ranges.pop_head(), None);

        let mut ranges = new_block_ranges([1..=4, 6..=8, 10..=10]);
        assert_eq!(ranges.pop_head(), Some(10));
        assert_eq!(ranges.pop_head(), Some(8));
        assert_eq!(ranges.pop_head(), Some(7));
        assert_eq!(ranges.pop_head(), Some(6));
        assert_eq!(ranges.pop_head(), Some(4));
        assert_eq!(ranges.pop_head(), Some(3));
        assert_eq!(ranges.pop_head(), Some(2));
        assert_eq!(ranges.pop_head(), Some(1));
        assert_eq!(ranges.pop_head(), None);
    }

    #[test]
    fn validate_check() {
        (1..=1).validate().unwrap();
        (1..=2).validate().unwrap();
        (0..=0).validate().unwrap_err();
        (0..=1).validate().unwrap_err();
        (2..=1).validate().unwrap_err();
    }

    #[test]
    fn adjacent_check() {
        assert!((3..=5).is_adjacent(&(1..=2)));
        assert!((3..=5).is_adjacent(&(6..=8)));

        assert!(!(3..=5).is_adjacent(&(1..=1)));
        assert!(!(3..=5).is_adjacent(&(7..=8)));
    }

    #[test]
    fn overlapping_check() {
        // equal
        assert!((3..=5).is_overlapping(&(3..=5)));

        // partial set
        assert!((3..=5).is_overlapping(&(1..=4)));
        assert!((3..=5).is_overlapping(&(1..=3)));
        assert!((3..=5).is_overlapping(&(4..=8)));
        assert!((3..=5).is_overlapping(&(5..=8)));

        // subset
        assert!((3..=5).is_overlapping(&(4..=4)));
        assert!((3..=5).is_overlapping(&(3..=4)));
        assert!((3..=5).is_overlapping(&(4..=5)));

        // superset
        assert!((3..=5).is_overlapping(&(1..=5)));
        assert!((3..=5).is_overlapping(&(1..=6)));
        assert!((3..=5).is_overlapping(&(3..=6)));
        assert!((3..=5).is_overlapping(&(4..=6)));

        // not overlapping
        assert!(!(3..=5).is_overlapping(&(1..=1)));
        assert!(!(3..=5).is_overlapping(&(7..=8)));
    }

    #[test]
    fn find_intersecting_ranges() {
        let ranges = new_block_ranges([30..=50, 80..=100, 130..=150]);

        assert_eq!(ranges.find_ranges(28..=28, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(1..=15, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(1..=28, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(3..=28, Strategy::Intersecting), None);
        assert_eq!(
            ranges.find_ranges(1..=29, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(1..=30, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(1..=49, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(1..=50, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(1..=51, Strategy::Intersecting),
            Some((0, 0))
        );

        assert_eq!(
            ranges.find_ranges(40..=51, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(50..=51, Strategy::Intersecting),
            Some((0, 0))
        );
        assert_eq!(
            ranges.find_ranges(51..=51, Strategy::Intersecting),
            Some((0, 0))
        );

        assert_eq!(
            ranges.find_ranges(40..=79, Strategy::Intersecting),
            Some((0, 1))
        );
        assert_eq!(
            ranges.find_ranges(50..=79, Strategy::Intersecting),
            Some((0, 1))
        );
        assert_eq!(
            ranges.find_ranges(51..=79, Strategy::Intersecting),
            Some((0, 1))
        );
        assert_eq!(
            ranges.find_ranges(50..=80, Strategy::Intersecting),
            Some((0, 1))
        );

        assert_eq!(ranges.find_ranges(52..=52, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(52..=78, Strategy::Intersecting), None);
        assert_eq!(
            ranges.find_ranges(52..=79, Strategy::Intersecting),
            Some((1, 1))
        );
        assert_eq!(
            ranges.find_ranges(52..=80, Strategy::Intersecting),
            Some((1, 1))
        );
        assert_eq!(
            ranges.find_ranges(52..=129, Strategy::Intersecting),
            Some((1, 2))
        );
        assert_eq!(
            ranges.find_ranges(99..=129, Strategy::Intersecting),
            Some((1, 2))
        );
        assert_eq!(
            ranges.find_ranges(100..=129, Strategy::Intersecting),
            Some((1, 2))
        );
        assert_eq!(
            ranges.find_ranges(101..=129, Strategy::Intersecting),
            Some((1, 2))
        );
        assert_eq!(
            ranges.find_ranges(101..=128, Strategy::Intersecting),
            Some((1, 1))
        );
        assert_eq!(
            ranges.find_ranges(51..=129, Strategy::Intersecting),
            Some((0, 2))
        );

        assert_eq!(ranges.find_ranges(102..=128, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(102..=120, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(120..=128, Strategy::Intersecting), None);

        assert_eq!(
            ranges.find_ranges(40..=129, Strategy::Intersecting),
            Some((0, 2))
        );
        assert_eq!(
            ranges.find_ranges(40..=140, Strategy::Intersecting),
            Some((0, 2))
        );
        assert_eq!(
            ranges.find_ranges(20..=140, Strategy::Intersecting),
            Some((0, 2))
        );
        assert_eq!(
            ranges.find_ranges(20..=150, Strategy::Intersecting),
            Some((0, 2))
        );
        assert_eq!(
            ranges.find_ranges(20..=151, Strategy::Intersecting),
            Some((0, 2))
        );
        assert_eq!(
            ranges.find_ranges(20..=160, Strategy::Intersecting),
            Some((0, 2))
        );

        assert_eq!(
            ranges.find_ranges(120..=129, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(ranges.find_ranges(120..=128, Strategy::Intersecting), None);
        assert_eq!(
            ranges.find_ranges(120..=130, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(
            ranges.find_ranges(120..=131, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(
            ranges.find_ranges(140..=145, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(
            ranges.find_ranges(140..=150, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(
            ranges.find_ranges(140..=155, Strategy::Intersecting),
            Some((2, 2))
        );
        assert_eq!(ranges.find_ranges(152..=155, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(152..=178, Strategy::Intersecting), None);
        assert_eq!(ranges.find_ranges(152..=152, Strategy::Intersecting), None);

        assert_eq!(
            new_block_ranges([]).find_ranges(1..=1, Strategy::Intersecting),
            None
        );

        assert_eq!(
            new_block_ranges([1..=2]).find_ranges(6..=9, Strategy::Intersecting),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8]).find_ranges(1..=2, Strategy::Intersecting),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_ranges(1..=2, Strategy::Intersecting),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_ranges(10..=12, Strategy::Intersecting),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_ranges(32..=32, Strategy::Intersecting),
            None
        );
    }

    #[test]
    fn insert_relaxed_disjoined() {
        let mut r = BlockRanges::default();
        r.insert_relaxed(10..=10).unwrap();
        assert_eq!(&r.0[..], &[10..=10][..]);

        let ranges = new_block_ranges([30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=1).unwrap();
        assert_eq!(&r.0[..], &[1..=1, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=28).unwrap();
        assert_eq!(&r.0[..], &[1..=28, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(10..=28).unwrap();
        assert_eq!(&r.0[..], &[10..=28, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(52..=78).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 52..=78, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(102..=128).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 102..=128, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(152..=152).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 152..=152][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(152..=170).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 152..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(160..=170).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 160..=170][..]);
    }

    #[test]
    fn insert_relaxed_intersected() {
        let ranges = new_block_ranges([30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=29).unwrap();
        assert_eq!(&r.0[..], &[29..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=29).unwrap();
        assert_eq!(&r.0[..], &[1..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=35).unwrap();
        assert_eq!(&r.0[..], &[29..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=55).unwrap();
        assert_eq!(&r.0[..], &[29..=55, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=78).unwrap();
        assert_eq!(&r.0[..], &[29..=78, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=79).unwrap();
        assert_eq!(&r.0[..], &[29..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(30..=79).unwrap();
        assert_eq!(&r.0[..], &[30..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(30..=150).unwrap();
        assert_eq!(&r.0[..], &[30..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(10..=170).unwrap();
        assert_eq!(&r.0[..], &[10..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(85..=129).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(85..=129).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(135..=170).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(151..=170).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=170][..]);

        let mut r = new_block_ranges([1..=2, 4..=6, 8..=10, 15..=20, 80..=100]);
        r.insert_relaxed(3..=79).unwrap();
        assert_eq!(&r.0[..], &[1..=100][..]);
    }

    #[test]
    fn remove_relaxed() {
        let ranges = new_block_ranges([30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.remove_relaxed(29..=29).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(30..=30).unwrap();
        assert_eq!(&r.0[..], &[31..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(20..=40).unwrap();
        assert_eq!(&r.0[..], &[41..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=40).unwrap();
        assert_eq!(&r.0[..], &[30..=34, 41..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(51..=129).unwrap();
        assert_eq!(&r.0[..], &[30..=50, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(50..=130).unwrap();
        assert_eq!(&r.0[..], &[30..=49, 131..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=49).unwrap();
        assert_eq!(&r.0[..], &[30..=34, 50..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=50).unwrap();
        assert_eq!(&r.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=55).unwrap();
        assert_eq!(&r.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=135).unwrap();
        assert_eq!(&r.0[..], &[30..=34, 136..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=170).unwrap();
        assert_eq!(&r.0[..], &[30..=34][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(10..=135).unwrap();
        assert_eq!(&r.0[..], &[136..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(10..=170).unwrap();
        assert!(r.0.is_empty());

        let mut r = new_block_ranges([1..=10, 12..=12, 14..=14]);
        r.remove_relaxed(12..=12).unwrap();
        assert_eq!(&r.0[..], &[1..=10, 14..=14][..]);

        let mut r = new_block_ranges([1..=u64::MAX]);
        r.remove_relaxed(12..=12).unwrap();
        assert_eq!(&r.0[..], &[1..=11, 13..=u64::MAX][..]);

        let mut r = new_block_ranges([1..=u64::MAX]);
        r.remove_relaxed(1..=1).unwrap();
        assert_eq!(&r.0[..], &[2..=u64::MAX][..]);

        let mut r = new_block_ranges([1..=u64::MAX]);
        r.remove_relaxed(u64::MAX..=u64::MAX).unwrap();
        assert_eq!(&r.0[..], &[1..=u64::MAX - 1][..]);
    }
}
