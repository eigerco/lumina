//! Ranges utilities

use std::borrow::Borrow;
use std::fmt::{Debug, Display};
use std::ops::{Add, RangeInclusive, Sub, SubAssign};

use serde::Serialize;
use smallvec::SmallVec;

/// Type alias to `RangeInclusive<u64>`.
pub type BlockRange = RangeInclusive<u64>;

/// Errors that can be produced by `BlockRanges`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BlockRangesError {
    /// Block ranges must be sorted.
    #[error("Block ranges are not sorted")]
    UnsortedBlockRanges,

    /// Not a valid block range.
    #[error("Invalid block range: {}", .0.display())]
    InvalidBlockRange(BlockRange),

    /// Provided range overlaps with another one.
    #[error("Insertion constrain not satisfied: Provided range ({}) overlaps with {}", .0.display(), .1.display())]
    BlockRangeOverlap(BlockRange, BlockRange),

    /// Provided range do not have any adjacent neightbors.
    #[error(
        "Insertion constrain not satified: Provided range ({}) do not have any adjacent neighbors", .0.display()
    )]
    NoAdjacentNeighbors(BlockRange),
}

type Result<T, E = BlockRangesError> = std::result::Result<T, E>;

pub(crate) trait BlockRangeExt {
    fn display(&self) -> BlockRangeDisplay;
    fn validate(&self) -> Result<()>;
    fn len(&self) -> u64;
    fn is_adjacent(&self, other: &BlockRange) -> bool;
    fn is_overlapping(&self, other: &BlockRange) -> bool;
    fn left_of(&self, other: &BlockRange) -> bool;
}

pub(crate) struct BlockRangeDisplay<'a>(&'a RangeInclusive<u64>);

impl<'a> Display for BlockRangeDisplay<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.0.start(), self.0.end())
    }
}

impl BlockRangeExt for BlockRange {
    fn display(&self) -> BlockRangeDisplay {
        BlockRangeDisplay(self)
    }

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

        // `self` overlaps with start of `other`
        //
        // self:  |------|
        // other:     |------|
        if self.start() < other.start() && other.contains(self.end()) {
            return true;
        }

        // `self` overlaps with end of `other`
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

    /// Returns `true` if the whole range of `self` is on the left of `other`.
    fn left_of(&self, other: &BlockRange) -> bool {
        debug_assert!(self.validate().is_ok());
        debug_assert!(other.validate().is_ok());
        self.end() < other.start()
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
        let ranges = BlockRanges::from_vec(raw_ranges)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        Ok(ranges)
    }
}

impl BlockRanges {
    /// Create a new, empty `BlockRanges`.
    pub fn new() -> BlockRanges {
        BlockRanges(SmallVec::new())
    }

    /// Create a `BlockRanges` from a [`SmallVec`].
    pub fn from_vec(ranges: SmallVec<[BlockRange; 2]>) -> Result<Self> {
        let mut prev: Option<&RangeInclusive<u64>> = None;

        for range in &ranges {
            range.validate()?;

            if let Some(prev) = prev {
                if range.start() <= prev.end() {
                    return Err(BlockRangesError::UnsortedBlockRanges);
                }
            }

            prev = Some(range);
        }

        Ok(BlockRanges(ranges))
    }

    /// Returns internal representation.
    pub fn into_inner(self) -> SmallVec<[BlockRange; 2]> {
        self.0
    }

    /// Return whether `BlockRanges` contains provided height.
    pub fn contains(&self, height: u64) -> bool {
        self.0.iter().any(|r| r.contains(&height))
    }

    /// Return whether range is empty.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|r| r.is_empty())
    }

    /// Return highest height in the range.
    pub fn head(&self) -> Option<u64> {
        self.0.last().map(|r| *r.end())
    }

    /// Return lowest height in the range.
    pub fn tail(&self) -> Option<u64> {
        self.0.first().map(|r| *r.start())
    }

    /// Returns first and last index of ranges overlapping or touching provided `range`.
    fn find_affected_ranges(&self, range: impl Borrow<BlockRange>) -> Option<(usize, usize)> {
        let range = range.borrow();
        debug_assert!(range.validate().is_ok());

        let mut start_idx = None;
        let mut end_idx = None;

        for (i, r) in self.0.iter().enumerate() {
            if r.is_overlapping(range) || r.is_adjacent(range) {
                if start_idx.is_none() {
                    start_idx = Some(i);
                }

                end_idx = Some(i);
            } else if end_idx.is_some() {
                // Ranges are sorted, we can skip checking the rest.
                break;
            }
        }

        Some((start_idx?, end_idx?))
    }

    /// Checks if `to_insert` is valid range to be inserted into the header store.
    ///
    /// This *must* be used when implementing [`Store::insert`].
    ///
    /// The constraints are as follows:
    ///
    /// * Insertion range must be valid.
    /// * Insertion range must not overlap with any of the existing ranges.
    /// * Insertion range must be appended to the left or to the right of an existing range.
    /// * Insertion is always allowed on empty `BlockRanges`.
    /// * New HEAD range can be created at the front if it doesn't overlap with existing one.
    ///
    /// Returns:
    ///
    /// * `Err(_)` if constraints are not met.
    /// * `Ok(true, false)` if `to_insert` range is going to be merged with its left neighbor.
    /// * `Ok(false, true)` if `to_insert` range is going to be merged with the right neighbor.
    /// * `Ok(true, true)` if `to_insert` range is going to be merged with both neighbors.
    /// * `Ok(false, false)` if `to_insert` range is going to be inserted as a new HEAD range (without merging).
    ///
    /// [`Store::insert`]: crate::store::Store::insert
    pub fn check_insertion_constraints(
        &self,
        to_insert: impl Borrow<BlockRange>,
    ) -> Result<(bool, bool)> {
        let to_insert = to_insert.borrow();
        to_insert.validate()?;

        let Some(head_range) = self.0.last() else {
            // Allow insersion on empty store.
            return Ok((false, false));
        };

        if head_range.left_of(to_insert) {
            // Allow adding a new HEAD
            let prev_exists = head_range.is_adjacent(to_insert);
            return Ok((prev_exists, false));
        }

        let Some((first_idx, last_idx)) = self.find_affected_ranges(to_insert) else {
            return Err(BlockRangesError::NoAdjacentNeighbors(to_insert.to_owned()));
        };

        let first = &self.0[first_idx];
        let last = &self.0[last_idx];
        let num_of_ranges = last_idx - first_idx + 1;

        match num_of_ranges {
            0 => unreachable!(),
            1 => {
                if first.is_overlapping(to_insert) {
                    Err(BlockRangesError::BlockRangeOverlap(
                        to_insert.to_owned(),
                        calc_overlap(to_insert, first, last),
                    ))
                } else if first.left_of(to_insert) {
                    Ok((true, false))
                } else {
                    Ok((false, true))
                }
            }
            2 => {
                if first.is_adjacent(to_insert) && last.is_adjacent(to_insert) {
                    Ok((true, true))
                } else {
                    Err(BlockRangesError::BlockRangeOverlap(
                        to_insert.to_owned(),
                        calc_overlap(to_insert, first, last),
                    ))
                }
            }
            _ => Err(BlockRangesError::BlockRangeOverlap(
                to_insert.to_owned(),
                calc_overlap(to_insert, first, last),
            )),
        }
    }

    /// Returns the head height and removes it from the ranges.
    pub fn pop_head(&mut self) -> Option<u64> {
        let last = self.0.last_mut()?;
        let head = *last.end();

        if last.len() == 1 {
            self.0.remove(self.0.len() - 1);
        } else {
            *last = *last.start()..=*last.end() - 1;
        }

        Some(head)
    }

    /// Returns the tail (lowest) height and removes it from the ranges.
    pub fn pop_tail(&mut self) -> Option<u64> {
        let first = self.0.first_mut()?;
        let tail = *first.start();

        if first.len() == 1 {
            self.0.remove(0);
        } else {
            *first = *first.start() + 1..=*first.end();
        }

        Some(tail)
    }

    /// Insert a new range.
    ///
    /// This fails only if `range` is not valid. It allows inserting an overlapping range.
    pub fn insert_relaxed(&mut self, range: impl Borrow<BlockRange>) -> Result<()> {
        let range = range.borrow();
        range.validate()?;

        match self.find_affected_ranges(range) {
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
                        self.0.insert(i, range.to_owned());
                        return Ok(());
                    }
                }

                self.0.push(range.to_owned());
            }
        }

        Ok(())
    }

    /// Remove a range.
    ///
    /// This fails only if `range` is not valid. It allows removing non-existing range.
    pub fn remove_relaxed(&mut self, range: impl Borrow<BlockRange>) -> Result<()> {
        let range = range.borrow();
        range.validate()?;

        let Some((start_idx, end_idx)) = self.find_affected_ranges(range) else {
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

impl Add for BlockRanges {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.add(&rhs)
    }
}

impl Add<&BlockRanges> for BlockRanges {
    type Output = Self;

    fn add(mut self, rhs: &BlockRanges) -> Self::Output {
        for range in rhs.0.iter() {
            self.insert_relaxed(range)
                .expect("BlockRanges always holds valid ranges");
        }
        self
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
            self.remove_relaxed(range)
                .expect("BlockRanges always holds valid ranges");
        }
        self
    }
}

impl Sub<RangeInclusive<u64>> for BlockRanges {
    type Output = Self;

    fn sub(mut self, range: RangeInclusive<u64>) -> Self::Output {
        if !range.is_empty() {
            self.remove_relaxed(range).expect("shouldn't panic");
        }
        self
    }
}

impl SubAssign<RangeInclusive<u64>> for BlockRanges {
    fn sub_assign(&mut self, range: RangeInclusive<u64>) {
        if !range.is_empty() {
            self.remove_relaxed(range).expect("shouldn't panic");
        }
    }
}

impl Iterator for BlockRanges {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        self.pop_tail()
    }
}

impl Display for BlockRanges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for (idx, range) in self.0.iter().enumerate() {
            if idx == 0 {
                write!(f, "{}", range.display())?;
            } else {
                write!(f, ", {}", range.display())?;
            }
        }
        write!(f, "]")
    }
}

fn calc_overlap(
    to_insert: &BlockRange,
    first_range: &BlockRange,
    last_range: &BlockRange,
) -> BlockRange {
    let start = first_range.start().max(to_insert.start());
    let end = last_range.end().min(to_insert.end());
    *start..=*end
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::new_block_ranges;

    #[test]
    fn range_len() {
        assert_eq!((0u64..=0).len(), 1);
        assert_eq!((0u64..=5).len(), 6);
        assert_eq!((1u64..=2).len(), 2);
        assert_eq!(RangeInclusive::new(2u64, 1).len(), 0);
        assert_eq!((10001u64..=20000).len(), 10000);
    }

    #[test]
    fn block_ranges_empty() {
        assert!(new_block_ranges([]).is_empty());
        assert!(!new_block_ranges([1..=3]).is_empty());
    }

    #[test]
    fn block_ranges_head() {
        assert_eq!(new_block_ranges([]).head(), None);
        assert_eq!(new_block_ranges([1..=3]).head(), Some(3));
        assert_eq!(new_block_ranges([1..=3, 6..=9]).head(), Some(9));
        assert_eq!(new_block_ranges([1..=3, 5..=5, 8..=9]).head(), Some(9));
    }

    #[test]
    fn block_ranges_tail() {
        assert_eq!(new_block_ranges([]).tail(), None);
        assert_eq!(new_block_ranges([1..=3]).tail(), Some(1));
        assert_eq!(new_block_ranges([1..=3, 6..=9]).tail(), Some(1));
        assert_eq!(new_block_ranges([1..=3, 5..=5, 8..=9]).tail(), Some(1));
    }

    #[test]
    fn check_range_insert_append() {
        let (prev_exists, next_exists) = new_block_ranges([])
            .check_insertion_constraints(1..=5)
            .unwrap();
        assert!(!prev_exists);
        assert!(!next_exists);

        let (prev_exists, next_exists) = new_block_ranges([1..=4])
            .check_insertion_constraints(5..=5)
            .unwrap();
        assert!(prev_exists);
        assert!(!next_exists);

        let (prev_exists, next_exists) = new_block_ranges([1..=5])
            .check_insertion_constraints(6..=9)
            .unwrap();
        assert!(prev_exists);
        assert!(!next_exists);

        let (prev_exists, next_exists) = new_block_ranges([6..=8])
            .check_insertion_constraints(2..=5)
            .unwrap();
        assert!(!prev_exists);
        assert!(next_exists);

        // Allow inserting a new HEAD range
        let (prev_exists, next_exists) = new_block_ranges([1..=5])
            .check_insertion_constraints(7..=9)
            .unwrap();
        assert!(!prev_exists);
        assert!(!next_exists);
    }

    #[test]
    fn check_range_insert_with_consolidation() {
        let (prev_exists, next_exists) = new_block_ranges([1..=3, 6..=9])
            .check_insertion_constraints(4..=5)
            .unwrap();
        assert!(prev_exists);
        assert!(next_exists);

        let (prev_exists, next_exists) = new_block_ranges([1..=2, 5..=5, 8..=9])
            .check_insertion_constraints(3..=4)
            .unwrap();
        assert!(prev_exists);
        assert!(next_exists);

        let (prev_exists, next_exists) = new_block_ranges([1..=2, 4..=4, 8..=9])
            .check_insertion_constraints(5..=7)
            .unwrap();
        assert!(prev_exists);
        assert!(next_exists);
    }

    #[test]
    fn check_range_insert_overlapping() {
        let result = new_block_ranges([1..=2])
            .check_insertion_constraints(1..=1)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(1..=1, 1..=1));

        let result = new_block_ranges([1..=2])
            .check_insertion_constraints(2..=2)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(2..=2, 2..=2));

        let result = new_block_ranges([1..=4])
            .check_insertion_constraints(2..=8)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(2..=8, 2..=4));

        let result = new_block_ranges([1..=4])
            .check_insertion_constraints(2..=3)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(2..=3, 2..=3));

        let result = new_block_ranges([5..=9])
            .check_insertion_constraints(1..=5)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(1..=5, 5..=5));

        let result = new_block_ranges([5..=8])
            .check_insertion_constraints(2..=8)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(2..=8, 5..=8));

        let result = new_block_ranges([1..=3, 6..=9])
            .check_insertion_constraints(3..=6)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(3..=6, 3..=6));

        let result = new_block_ranges([1..=3, 5..=6])
            .check_insertion_constraints(3..=9)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(3..=9, 3..=6));

        let result = new_block_ranges([2..=3, 5..=6])
            .check_insertion_constraints(1..=5)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::BlockRangeOverlap(1..=5, 2..=5));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = new_block_ranges([1..=2, 7..=9])
            .check_insertion_constraints(4..=4)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::NoAdjacentNeighbors(4..=4));

        let result = new_block_ranges([1..=2, 8..=9])
            .check_insertion_constraints(4..=6)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::NoAdjacentNeighbors(4..=6));

        let result = new_block_ranges([4..=5, 7..=8])
            .check_insertion_constraints(1..=2)
            .unwrap_err();
        assert_eq!(result, BlockRangesError::NoAdjacentNeighbors(1..=2));
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
    fn pop_tail() {
        let mut ranges = new_block_ranges([]);
        assert_eq!(ranges.pop_tail(), None);

        let mut ranges = new_block_ranges([1..=4, 6..=8, 10..=10]);
        assert_eq!(ranges.pop_tail(), Some(1));
        assert_eq!(ranges.pop_tail(), Some(2));
        assert_eq!(ranges.pop_tail(), Some(3));
        assert_eq!(ranges.pop_tail(), Some(4));
        assert_eq!(ranges.pop_tail(), Some(6));
        assert_eq!(ranges.pop_tail(), Some(7));
        assert_eq!(ranges.pop_tail(), Some(8));
        assert_eq!(ranges.pop_tail(), Some(10));
        assert_eq!(ranges.pop_tail(), None);
    }

    #[test]
    fn block_ranges_iterator() {
        let ranges = new_block_ranges([1..=5, 10..=15]);
        let heights: Vec<_> = ranges.into_iter().collect();
        assert_eq!(heights, vec![1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 15]);

        let empty_heights: Vec<u64> = new_block_ranges([]).into_iter().collect();
        assert_eq!(empty_heights, Vec::<u64>::new())
    }


    #[test]
    fn validate_check() {
        (1..=1).validate().unwrap();
        (1..=2).validate().unwrap();
        (0..=0).validate().unwrap_err();
        (0..=1).validate().unwrap_err();
        #[allow(clippy::reversed_empty_ranges)]
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
    fn left_of_check() {
        // range is on the left of
        assert!((1..=2).left_of(&(3..=4)));
        assert!((1..=1).left_of(&(3..=4)));

        // range is on the right of
        assert!(!(3..=4).left_of(&(1..=2)));
        assert!(!(3..=4).left_of(&(1..=1)));

        // overlapping is not accepted
        assert!(!(1..=3).left_of(&(3..=4)));
        assert!(!(1..=5).left_of(&(3..=4)));
        assert!(!(3..=4).left_of(&(1..=3)));
    }

    #[test]
    fn find_affected_ranges() {
        let ranges = new_block_ranges([30..=50, 80..=100, 130..=150]);

        assert_eq!(ranges.find_affected_ranges(28..=28), None);
        assert_eq!(ranges.find_affected_ranges(1..=15), None);
        assert_eq!(ranges.find_affected_ranges(1..=28), None);
        assert_eq!(ranges.find_affected_ranges(3..=28), None);
        assert_eq!(ranges.find_affected_ranges(1..=29), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(1..=30), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(1..=49), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(1..=50), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(1..=51), Some((0, 0)));

        assert_eq!(ranges.find_affected_ranges(40..=51), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(50..=51), Some((0, 0)));
        assert_eq!(ranges.find_affected_ranges(51..=51), Some((0, 0)));

        assert_eq!(ranges.find_affected_ranges(40..=79), Some((0, 1)));
        assert_eq!(ranges.find_affected_ranges(50..=79), Some((0, 1)));
        assert_eq!(ranges.find_affected_ranges(51..=79), Some((0, 1)));
        assert_eq!(ranges.find_affected_ranges(50..=80), Some((0, 1)));

        assert_eq!(ranges.find_affected_ranges(52..=52), None);
        assert_eq!(ranges.find_affected_ranges(52..=78), None);
        assert_eq!(ranges.find_affected_ranges(52..=79), Some((1, 1)));
        assert_eq!(ranges.find_affected_ranges(52..=80), Some((1, 1)));
        assert_eq!(ranges.find_affected_ranges(52..=129), Some((1, 2)));
        assert_eq!(ranges.find_affected_ranges(99..=129), Some((1, 2)));
        assert_eq!(ranges.find_affected_ranges(100..=129), Some((1, 2)));
        assert_eq!(ranges.find_affected_ranges(101..=129), Some((1, 2)));
        assert_eq!(ranges.find_affected_ranges(101..=128), Some((1, 1)));
        assert_eq!(ranges.find_affected_ranges(51..=129), Some((0, 2)));

        assert_eq!(ranges.find_affected_ranges(102..=128), None);
        assert_eq!(ranges.find_affected_ranges(102..=120), None);
        assert_eq!(ranges.find_affected_ranges(120..=128), None);

        assert_eq!(ranges.find_affected_ranges(40..=129), Some((0, 2)));
        assert_eq!(ranges.find_affected_ranges(40..=140), Some((0, 2)));
        assert_eq!(ranges.find_affected_ranges(20..=140), Some((0, 2)));
        assert_eq!(ranges.find_affected_ranges(20..=150), Some((0, 2)));
        assert_eq!(ranges.find_affected_ranges(20..=151), Some((0, 2)));
        assert_eq!(ranges.find_affected_ranges(20..=160), Some((0, 2)));

        assert_eq!(ranges.find_affected_ranges(120..=129), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(120..=128), None);
        assert_eq!(ranges.find_affected_ranges(120..=130), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(120..=131), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(140..=145), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(140..=150), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(140..=155), Some((2, 2)));
        assert_eq!(ranges.find_affected_ranges(152..=155), None);
        assert_eq!(ranges.find_affected_ranges(152..=178), None);
        assert_eq!(ranges.find_affected_ranges(152..=152), None);

        assert_eq!(new_block_ranges([]).find_affected_ranges(1..=1), None);

        assert_eq!(new_block_ranges([1..=2]).find_affected_ranges(6..=9), None);
        assert_eq!(new_block_ranges([4..=8]).find_affected_ranges(1..=2), None);
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_affected_ranges(1..=2),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_affected_ranges(10..=12),
            None
        );
        assert_eq!(
            new_block_ranges([4..=8, 20..=30]).find_affected_ranges(32..=32),
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

    #[test]
    fn sub() {
        let minuend = new_block_ranges([30..=50, 80..=100, 130..=150]);

        let difference = minuend.clone() - (29..=29);
        assert_eq!(&difference.0[..], &[30..=50, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (30..=30);
        assert_eq!(&difference.0[..], &[31..=50, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (20..=40);
        assert_eq!(&difference.0[..], &[41..=50, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (35..=40);
        assert_eq!(&difference.0[..], &[30..=34, 41..=50, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (51..=129);
        assert_eq!(&difference.0[..], &[30..=50, 130..=150][..]);

        let difference = minuend.clone() - (50..=130);
        assert_eq!(&difference.0[..], &[30..=49, 131..=150][..]);

        let difference = minuend.clone() - (35..=49);
        assert_eq!(&difference.0[..], &[30..=34, 50..=50, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (35..=50);
        assert_eq!(&difference.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (35..=55);
        assert_eq!(&difference.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let difference = minuend.clone() - (35..=135);
        assert_eq!(&difference.0[..], &[30..=34, 136..=150][..]);

        let difference = minuend.clone() - (35..=170);
        assert_eq!(&difference.0[..], &[30..=34][..]);

        let difference = minuend.clone() - (10..=135);
        assert_eq!(&difference.0[..], &[136..=150][..]);

        let difference = minuend.clone() - (10..=170);
        assert!(difference.0.is_empty());

        let difference = minuend.clone() - (170..=10);
        assert_eq!(&difference.0[..], &minuend.0[..]);

        let minuend = new_block_ranges([1..=10, 12..=12, 14..=14]);
        let difference = minuend - (12..=12);
        assert_eq!(&difference.0[..], &[1..=10, 14..=14][..]);

        let minuend = new_block_ranges([1..=u64::MAX]);
        let difference = minuend - (12..=12);
        assert_eq!(&difference.0[..], &[1..=11, 13..=u64::MAX][..]);

        let minuend = new_block_ranges([1..=u64::MAX]);
        let difference = minuend - (1..=1);
        assert_eq!(&difference.0[..], &[2..=u64::MAX][..]);

        let minuend = new_block_ranges([1..=u64::MAX]);
        let difference = minuend - (u64::MAX..=u64::MAX);
        assert_eq!(&difference.0[..], &[1..=u64::MAX - 1][..]);
    }

    #[test]
    fn sub_assign() {
        let minuend = new_block_ranges([30..=50, 80..=100, 130..=150]);

        let mut difference = minuend.clone();
        difference -= 29..=29;
        assert_eq!(&difference.0[..], &[30..=50, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 30..=30;
        assert_eq!(&difference.0[..], &[31..=50, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 20..=40;
        assert_eq!(&difference.0[..], &[41..=50, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=40;
        assert_eq!(&difference.0[..], &[30..=34, 41..=50, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 51..=129;
        assert_eq!(&difference.0[..], &[30..=50, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 50..=130;
        assert_eq!(&difference.0[..], &[30..=49, 131..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=49;
        assert_eq!(&difference.0[..], &[30..=34, 50..=50, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=50;
        assert_eq!(&difference.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=55;
        assert_eq!(&difference.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=135;
        assert_eq!(&difference.0[..], &[30..=34, 136..=150][..]);

        let mut difference = minuend.clone();
        difference -= 35..=170;
        assert_eq!(&difference.0[..], &[30..=34][..]);

        let mut difference = minuend.clone();
        difference -= 10..=135;
        assert_eq!(&difference.0[..], &[136..=150][..]);

        let mut difference = minuend.clone();
        difference -= 10..=170;
        assert!(difference.0.is_empty());

        let mut difference = minuend.clone();
        difference -= 170..=10;
        assert_eq!(&difference.0[..], &minuend.0[..]);

        let mut difference = new_block_ranges([1..=10, 12..=12, 14..=14]);
        difference -= 12..=12;
        assert_eq!(&difference.0[..], &[1..=10, 14..=14][..]);

        let mut difference = new_block_ranges([1..=u64::MAX]);
        difference -= 12..=12;
        assert_eq!(&difference.0[..], &[1..=11, 13..=u64::MAX][..]);

        let mut difference = new_block_ranges([1..=u64::MAX]);
        difference -= 1..=1;
        assert_eq!(&difference.0[..], &[2..=u64::MAX][..]);

        let mut difference = new_block_ranges([1..=u64::MAX]);
        difference -= u64::MAX..=u64::MAX;
        assert_eq!(&difference.0[..], &[1..=u64::MAX - 1][..]);

    }
}
