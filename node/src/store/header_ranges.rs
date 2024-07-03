use std::fmt::{self, Debug, Display};
use std::iter;
use std::mem;
use std::ops::{RangeBounds, RangeInclusive, Sub};
use std::vec;

use celestia_types::ExtendedHeader;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::store::utils::{ranges_intersection, try_consolidate_ranges, RangeScanResult};
use crate::store::StoreError;

pub type BlockRangeOld = RangeInclusive<u64>;

pub(crate) trait RangeLengthExt {
    fn len(&self) -> u64;
}

impl RangeLengthExt for BlockRangeOld {
    fn len(&self) -> u64 {
        match self.end().checked_sub(*self.start()) {
            Some(difference) => difference + 1,
            None => 0,
        }
    }
}

//pub type BlockRange = RangeInclusive<u64>;

#[derive(Debug, thiserror::Error)]
pub enum BlockRangesError {
    #[error("Invalid block range: {0}-{1}")]
    InvalidBlockRange(u64, u64),
}

type Result<T, E = BlockRangesError> = std::result::Result<T, E>;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct BlockRangeNew(RangeInclusive<u64>);

impl BlockRangeNew {
    pub fn new(start: u64, end: u64) -> Result<BlockRangeNew> {
        if start == 0 || start > end {
            Err(BlockRangesError::InvalidBlockRange(start, end))
        } else {
            Ok(BlockRangeNew(start..=end))
        }
    }

    pub fn start(&self) -> &u64 {
        self.0.start()
    }

    pub fn end(&self) -> &u64 {
        self.0.end()
    }

    pub fn contains(&self, value: &u64) -> bool {
        self.0.contains(value)
    }

    pub fn len(&self) -> u64 {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn is_adjacent(&self, other: &BlockRangeNew) -> bool {
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

    pub fn is_overlapping(&self, other: &BlockRangeNew) -> bool {
        // range1 is partial set of range2, case 1
        //
        // self:  |------|
        // other:     |------|
        if self.start() < other.start() && other.contains(self.end()) {
            return true;
        }

        // range1 is partial set of range2, case 2
        //
        // self:      |------|
        // other: |------|
        if self.end() > other.end() && other.contains(self.start()) {
            return true;
        }

        // range1 is subset of range2
        //
        // self:    |--|
        // other: |------|
        if self.start() >= other.start() && self.end() <= other.end() {
            return true;
        }

        // range1 is superset of range2
        //
        // self:  |------|
        // other:   |--|
        if self.start() <= other.start() && self.end() >= other.end() {
            return true;
        }

        false
    }
}

impl AsRef<RangeInclusive<u64>> for BlockRangeNew {
    fn as_ref(&self) -> &RangeInclusive<u64> {
        &self.0
    }
}

impl Display for BlockRangeNew {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.start(), self.end())
    }
}

impl Debug for BlockRangeNew {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl From<BlockRangeNew> for RangeInclusive<u64> {
    fn from(value: BlockRangeNew) -> Self {
        value.0
    }
}

impl Iterator for BlockRangeNew {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl DoubleEndedIterator for BlockRangeNew {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
}

impl TryFrom<RangeInclusive<u64>> for BlockRangeNew {
    type Error = BlockRangesError;

    fn try_from(value: RangeInclusive<u64>) -> Result<Self, Self::Error> {
        BlockRangeNew::new(*value.start(), *value.end())
    }
}

impl TryFrom<&RangeInclusive<u64>> for BlockRangeNew {
    type Error = BlockRangesError;

    fn try_from(value: &RangeInclusive<u64>) -> Result<Self, Self::Error> {
        BlockRangeNew::new(*value.start(), *value.end())
    }
}

impl PartialEq<RangeInclusive<u64>> for BlockRangeNew {
    fn eq(&self, other: &RangeInclusive<u64>) -> bool {
        PartialEq::eq(&self.0, other)
    }

    fn ne(&self, other: &RangeInclusive<u64>) -> bool {
        PartialEq::ne(&self.0, other)
    }
}

/// Represents possibly multiple non-overlapping, sorted ranges of header heights
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockRanges(SmallVec<[BlockRangeNew; 2]>);

pub(crate) trait BlockRangesExt {
    /// Check whether provided `to_insert` range can be inserted into the header ranges represented
    /// by self. New range can be inserted ahead of all existing ranges to allow syncing from the
    /// head but otherwise, only growing the existing ranges is allowed.
    /// Returned [`RangeScanResult`] contains information necessary to persist the range
    /// modification in the database manually, or one can call [`update_range`] to modify ranges in
    /// memory.
    fn check_range_insert(&self, to_insert: &BlockRangeOld) -> Result<RangeScanResult, StoreError>;
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
            self.0.push(BlockRangeNew(range));
        } else {
            self.0[range_index] = BlockRangeNew(range);
        }

        if let Some(to_remove) = range_to_remove {
            self.0.remove(to_remove);
        }
    }

    fn check_range_insert(&self, to_insert: &BlockRangeOld) -> Result<RangeScanResult, StoreError> {
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

            if let Some(intersection) = ranges_intersection(&stored_range.0, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(&stored_range.0, to_insert) {
                break RangeScanResult {
                    range_index: idx,
                    range: consolidated,
                    range_to_remove: None,
                };
            }
        };

        // we have a hit, check whether we can merge with the next range too
        if let Some((idx, range_after)) = stored_ranges_iter.next() {
            if let Some(intersection) = ranges_intersection(&range_after.0, to_insert) {
                return Err(StoreError::HeaderRangeOverlap(
                    *intersection.start(),
                    *intersection.end(),
                ));
            }

            if let Some(consolidated) = try_consolidate_ranges(&range_after.0, &found_range.range) {
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
    Overlapping,
    Adjacent,
    OverlappingOrAdjacent,
}

impl BlockRanges {
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

    pub(crate) fn into_inner(self) -> SmallVec<[BlockRangeNew; 2]> {
        self.0
    }

    fn find_intersecting_ranges(
        &self,
        range: impl TryInto<BlockRangeNew>,
    ) -> Option<(usize, usize)> {
        self.find_ranges(range, Strategy::OverlappingOrAdjacent)
    }

    /// Returns the start index and end index of an intersection.
    ///
    /// Intersection in our case means overlapping or touching of a range.
    fn find_ranges(&self, range: &BlockRangeNew, kind: Strategy) -> Option<(usize, usize)> {
        let mut start_idx = None;
        let mut end_idx = None;

        for (i, r) in self.0.iter().enumerate() {
            let found = match kind {
                Strategy::Overlapping => is_overlapping(r, &range),
                Strategy::Adjacent => is_touching(r, &range),
                Strategy::OverlappingOrAdjacent => {
                    is_overlapping(r, &range) || is_touching(r, &range)
                }
            };

            if found {
                if start_idx.is_none() {
                    start_idx = Some(i);
                }

                end_idx = Some(i);
            } else if end_idx.is_some() {
                // We reached from satisfying range to a non-satisfying.
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
                ranges.push(BlockRangeNew(next_start..=*range.start() - 1));
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
            *last = BlockRangeNew(*last.start()..=*last.end() - 1);
        }

        Some(head)
    }

    // TODO: what about 0?
    pub(crate) fn insert_relaxed(&mut self, range: impl TryInto<BlockRangeNew>) {
        let range = range.try_into().unwrap_or_else(|_| panic!("AA"));

        match self.find_intersecting_ranges(range.clone()) {
            // `range` must be merged with other ranges
            Some((start_idx, end_idx)) => {
                let start = *self.0[start_idx].start().min(range.start());
                let end = *self.0[end_idx].end().max(range.end());

                self.0.drain(start_idx..=end_idx);
                self.0.insert(start_idx, BlockRangeNew(start..=end));
            }
            // `range` can not be merged with other ranges
            None => {
                for (i, r) in self.0.iter().enumerate() {
                    if range.end() < r.start() {
                        self.0.insert(i, range);
                        return;
                    }
                }

                self.0.push(range);
            }
        }
    }

    pub(crate) fn remove_relaxed(&mut self, range: impl TryInto<BlockRangeNew>) {
        let range = range.try_into().unwrap_or_else(|_| panic!("AA"));

        let Some((start_idx, end_idx)) = self.find_ranges(range.clone(), Strategy::Overlapping)
        else {
            // Nothing to remove
            return;
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
            self.0.insert(
                start_idx,
                BlockRangeNew(*range.end() + 1..=*last_range.end()),
            );
        }

        if first_range.start() < range.start() {
            // Add the left range
            self.0.insert(
                start_idx,
                BlockRangeNew(*first_range.start()..=*range.start() - 1),
            );
        }
    }

    /// Crate BlockRanges from correctly pre-sorted, non-overlapping SmallVec of ranges
    pub(crate) fn from_vec(from: SmallVec<[BlockRangeOld; 2]>) -> Self {
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

        BlockRanges(from.into_iter().map(BlockRangeNew).collect())
    }
}

impl AsRef<[RangeInclusive<u64>]> for BlockRanges {
    fn as_ref(&self) -> &[RangeInclusive<u64>] {
        unsafe {
            // SAFETY: It is safe to transmute because of `repr(transparent)`.
            mem::transmute(self.0.as_ref())
        }
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
            self.remove_relaxed(range.to_owned());
        }
        self
    }
}

impl Display for BlockRanges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for (idx, range) in self.0.iter().enumerate() {
            if idx == 0 {
                write!(f, "{range}")?;
            } else {
                write!(f, ", {range}")?;
            }
        }
        write!(f, "]")
    }
}

fn is_touching(range1: &BlockRangeNew, range2: &BlockRangeNew) -> bool {
    // End of range1 touches start of range2
    //
    // range1: |------|
    // range2:         |------|
    if *range1.end() == range2.start().saturating_sub(1) {
        return true;
    }

    // Start of range1 touches end of range2
    //
    // range1:         |------|
    // range2: |------|
    if range1.start().saturating_sub(1) == *range2.end() {
        return true;
    }

    false
}

fn is_overlapping(range1: &BlockRangeNew, range2: &BlockRangeNew) -> bool {
    // range1 is partial set of range2, case 1
    //
    // range1: |------|
    // range2:     |------|
    if range1.start() < range2.start() && range2.contains(range1.end()) {
        return true;
    }

    // range1 is partial set of range2, case 2
    //
    // range1:     |------|
    // range2: |------|
    if range1.end() > range2.end() && range2.contains(range1.start()) {
        return true;
    }

    // range1 is subset of range2
    //
    // range1:   |--|
    // range2: |------|
    if range1.start() >= range2.start() && range1.end() <= range2.end() {
        return true;
    }

    // range1 is superset of range2
    //
    // range1: |------|
    // range2:   |--|
    if range1.start() <= range2.start() && range1.end() >= range2.end() {
        return true;
    }

    false
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
        assert!(BlockRanges::from_vec(smallvec![]).is_empty());
        assert!(!BlockRanges::from_vec(smallvec![1..=3]).is_empty());
    }

    #[test]
    fn header_ranges_head() {
        assert_eq!(BlockRanges::from_vec(smallvec![]).head(), None);
        assert_eq!(BlockRanges::from_vec(smallvec![1..=3]).head(), Some(3));
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=3, 6..=9]).head(),
            Some(9)
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=3, 5..=5, 8..=9]).head(),
            Some(9)
        );
    }

    #[test]
    fn header_ranges_tail() {
        assert_eq!(BlockRanges::from_vec(smallvec![]).tail(), None);
        assert_eq!(BlockRanges::from_vec(smallvec![1..=3]).tail(), Some(1));
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=3, 6..=9]).tail(),
            Some(1)
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=3, 5..=5, 8..=9]).tail(),
            Some(1)
        );
    }

    #[test]
    fn check_range_insert_append() {
        let result = BlockRanges::from_vec(smallvec![])
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

        let result = BlockRanges::from_vec(smallvec![1..=4])
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

        let result = BlockRanges::from_vec(smallvec![1..=5])
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

        let result = BlockRanges::from_vec(smallvec![6..=8])
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
        let result = BlockRanges::from_vec(smallvec![1..=3, 6..=9])
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

        let result = BlockRanges::from_vec(smallvec![1..=2, 5..=5, 8..=9])
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

        let result = BlockRanges::from_vec(smallvec![1..=2, 4..=4, 8..=9])
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
        let result = BlockRanges::from_vec(smallvec![1..=2])
            .check_range_insert(&(1..=1))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(1, 1)));

        let result = BlockRanges::from_vec(smallvec![1..=4])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 4)));

        let result = BlockRanges::from_vec(smallvec![1..=4])
            .check_range_insert(&(2..=3))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 3)));

        let result = BlockRanges::from_vec(smallvec![5..=9])
            .check_range_insert(&(1..=5))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 5)));

        let result = BlockRanges::from_vec(smallvec![5..=8])
            .check_range_insert(&(2..=8))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 8)));

        let result = BlockRanges::from_vec(smallvec![1..=3, 6..=9])
            .check_range_insert(&(3..=6))
            .unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(3, 3)));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = BlockRanges::from_vec(smallvec![1..=2, 7..=9])
            .check_range_insert(&(4..=4))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = BlockRanges::from_vec(smallvec![1..=2, 8..=9])
            .check_range_insert(&(4..=6))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = BlockRanges::from_vec(smallvec![4..=5, 7..=8])
            .check_range_insert(&(1..=2))
            .unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));
    }

    #[test]
    fn test_header_range_creation_ok() {
        BlockRanges::from_vec(smallvec![1..=3, 5..=8]);
        BlockRanges::from_vec(smallvec![]);
        BlockRanges::from_vec(smallvec![1..=1, 1000000..=2000000]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_overlap() {
        BlockRanges::from_vec(smallvec![1..=3, 2..=5]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_inverse() {
        BlockRanges::from_vec(smallvec![1..=3, RangeInclusive::new(9, 5)]);
    }

    #[test]
    #[should_panic]
    fn test_header_range_creation_wrong_order() {
        BlockRanges::from_vec(smallvec![10..=15, 1..=5]);
    }

    #[test]
    fn inverted() {
        assert_eq!(
            BlockRanges::default().inverted_up_to_head(),
            BlockRanges::default()
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=100]).inverted_up_to_head(),
            BlockRanges::default()
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![2..=100]).inverted_up_to_head(),
            BlockRanges::from_vec(smallvec![1..=1])
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![2..=100, 102..=102]).inverted_up_to_head(),
            BlockRanges::from_vec(smallvec![1..=1, 101..=101])
        );

        assert_eq!(
            BlockRanges::from_vec(smallvec![2..=100, 150..=160, 200..=210, 300..=310])
                .inverted_up_to_head(),
            BlockRanges::from_vec(smallvec![1..=1, 101..=149, 161..=199, 211..=299])
        );
    }

    #[test]
    fn pop_head() {
        let mut ranges = BlockRanges::from_vec(smallvec![]);
        assert_eq!(ranges.pop_head(), None);

        let mut ranges = BlockRanges::from_vec(smallvec![1..=4, 6..=8, 10..=10]);
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

    /*
    #[test]
    fn touching_check() {
        assert!(is_touching(&(3..=5), &(1..=2)));
        assert!(is_touching(&(3..=5), &(6..=8)));

        assert!(!is_touching(&(3..=5), &(1..=1)));
        assert!(!is_touching(&(3..=5), &(7..=8)));
    }

    #[test]
    fn overlapping_check() {
        // equal
        assert!(is_overlapping(&(3..=5), &(3..=5)));

        // partial set
        assert!(is_overlapping(&(3..=5), &(1..=4)));
        assert!(is_overlapping(&(3..=5), &(1..=3)));
        assert!(is_overlapping(&(3..=5), &(4..=8)));
        assert!(is_overlapping(&(3..=5), &(5..=8)));

        // subset
        assert!(is_overlapping(&(3..=5), &(4..=4)));
        assert!(is_overlapping(&(3..=5), &(3..=4)));
        assert!(is_overlapping(&(3..=5), &(4..=5)));

        // superset
        assert!(is_overlapping(&(3..=5), &(1..=5)));
        assert!(is_overlapping(&(3..=5), &(1..=6)));
        assert!(is_overlapping(&(3..=5), &(3..=6)));
        assert!(is_overlapping(&(3..=5), &(4..=6)));

        // not overlapping
        assert!(!is_overlapping(&(3..=5), &(1..=1)));
        assert!(!is_overlapping(&(3..=5), &(7..=8)));
    }
    */

    #[test]
    fn intersection_non_overlapping_non_touching() {
        assert_eq!(
            BlockRanges::from_vec(smallvec![]).find_intersecting_ranges(&(1..=1)),
            None
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![1..=2]).find_intersecting_ranges(&(6..=9)),
            None
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![4..=8]).find_intersecting_ranges(&(1..=2)),
            None
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![4..=8, 20..=30]).find_intersecting_ranges(&(1..=2)),
            None
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![4..=8, 20..=30]).find_intersecting_ranges(&(10..=12)),
            None
        );
        assert_eq!(
            BlockRanges::from_vec(smallvec![4..=8, 20..=30]).find_intersecting_ranges(&(32..=32)),
            None
        );

        let ranges = BlockRanges::from_vec(smallvec![30..=50, 80..=100, 130..=150]);
        assert_eq!(ranges.find_intersecting_ranges(&(28..=28)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(1..=15)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(1..=28)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(3..=28)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(52..=52)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(52..=78)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(102..=128)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(102..=120)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(120..=128)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(152..=178)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(152..=152)), None);
    }

    #[test]
    fn intersection_overlapping_or_touching() {
        let ranges = BlockRanges::from_vec(smallvec![30..=50, 80..=100, 130..=150]);

        assert_eq!(ranges.find_intersecting_ranges(&(1..=29)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(1..=30)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(1..=49)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(1..=50)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(1..=51)), Some((0, 0)));

        assert_eq!(ranges.find_intersecting_ranges(&(40..=51)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(50..=51)), Some((0, 0)));
        assert_eq!(ranges.find_intersecting_ranges(&(51..=51)), Some((0, 0)));

        assert_eq!(ranges.find_intersecting_ranges(&(40..=79)), Some((0, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(50..=79)), Some((0, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(51..=79)), Some((0, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(50..=80)), Some((0, 1)));

        assert_eq!(ranges.find_intersecting_ranges(&(52..=79)), Some((1, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(52..=80)), Some((1, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(52..=129)), Some((1, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(99..=129)), Some((1, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(100..=129)), Some((1, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(101..=129)), Some((1, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(101..=128)), Some((1, 1)));
        assert_eq!(ranges.find_intersecting_ranges(&(51..=129)), Some((0, 2)));

        assert_eq!(ranges.find_intersecting_ranges(&(40..=129)), Some((0, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(40..=140)), Some((0, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(20..=140)), Some((0, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(20..=150)), Some((0, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(20..=151)), Some((0, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(20..=160)), Some((0, 2)));

        assert_eq!(ranges.find_intersecting_ranges(&(120..=129)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(120..=128)), None);
        assert_eq!(ranges.find_intersecting_ranges(&(120..=130)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(120..=131)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(140..=145)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(140..=150)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(140..=155)), Some((2, 2)));
        assert_eq!(ranges.find_intersecting_ranges(&(152..=155)), None);
    }

    #[test]
    fn insert_relaxed_disjoined() {
        let mut r = BlockRanges::default();
        r.insert_relaxed(10..=10);
        assert_eq!(&r.0[..], &[10..=10][..]);

        let ranges = BlockRanges::from_vec(smallvec![30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=1);
        assert_eq!(&r.0[..], &[1..=1, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=28);
        assert_eq!(&r.0[..], &[1..=28, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(10..=28);
        assert_eq!(&r.0[..], &[10..=28, 30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(52..=78);
        assert_eq!(&r.0[..], &[30..=50, 52..=78, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(102..=128);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 102..=128, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(152..=152);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 152..=152][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(152..=170);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 152..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(160..=170);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150, 160..=170][..]);
    }

    #[test]
    fn insert_relaxed_intersected() {
        let ranges = BlockRanges::from_vec(smallvec![30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=29);
        assert_eq!(&r.0[..], &[29..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(1..=29);
        assert_eq!(&r.0[..], &[1..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=35);
        assert_eq!(&r.0[..], &[29..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=55);
        assert_eq!(&r.0[..], &[29..=55, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=78);
        assert_eq!(&r.0[..], &[29..=78, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(29..=79);
        assert_eq!(&r.0[..], &[29..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(30..=79);
        assert_eq!(&r.0[..], &[30..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(30..=150);
        assert_eq!(&r.0[..], &[30..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(10..=170);
        assert_eq!(&r.0[..], &[10..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(85..=129);
        assert_eq!(&r.0[..], &[30..=50, 80..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(85..=129);
        assert_eq!(&r.0[..], &[30..=50, 80..=150][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(135..=170);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=170][..]);

        let mut r = ranges.clone();
        r.insert_relaxed(151..=170);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=170][..]);

        let mut r = BlockRanges::from_vec(smallvec![1..=2, 4..=6, 8..=10, 15..=20, 80..=100]);
        r.insert_relaxed(3..=79);
        assert_eq!(&r.0[..], &[1..=100][..]);
    }

    #[test]
    fn remove_relaxed() {
        let ranges = BlockRanges::from_vec(smallvec![30..=50, 80..=100, 130..=150]);

        let mut r = ranges.clone();
        r.remove_relaxed(29..=29);
        assert_eq!(&r.0[..], &[30..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(30..=30);
        assert_eq!(&r.0[..], &[31..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(20..=40);
        assert_eq!(&r.0[..], &[41..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=40);
        assert_eq!(&r.0[..], &[30..=34, 41..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(51..=129);
        assert_eq!(&r.0[..], &[30..=50, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(50..=130);
        assert_eq!(&r.0[..], &[30..=49, 131..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=49);
        assert_eq!(&r.0[..], &[30..=34, 50..=50, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=50);
        assert_eq!(&r.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=55);
        assert_eq!(&r.0[..], &[30..=34, 80..=100, 130..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=135);
        assert_eq!(&r.0[..], &[30..=34, 136..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(35..=170);
        assert_eq!(&r.0[..], &[30..=34][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(10..=135);
        assert_eq!(&r.0[..], &[136..=150][..]);

        let mut r = ranges.clone();
        r.remove_relaxed(10..=170);
        assert!(r.0.is_empty());

        let mut r = BlockRanges::from_vec(smallvec![1..=10, 12..=12, 14..=14]);
        r.remove_relaxed(12..=12);
        assert_eq!(&r.0[..], &[1..=10, 14..=14][..]);

        let mut r = BlockRanges::from_vec(smallvec![1..=u64::MAX]);
        r.remove_relaxed(12..=12);
        assert_eq!(&r.0[..], &[1..=11, 13..=u64::MAX][..]);

        let mut r = BlockRanges::from_vec(smallvec![1..=u64::MAX]);
        r.remove_relaxed(1..=1);
        assert_eq!(&r.0[..], &[2..=u64::MAX][..]);

        let mut r = BlockRanges::from_vec(smallvec![1..=u64::MAX]);
        r.remove_relaxed(u64::MAX..=u64::MAX);
        assert_eq!(&r.0[..], &[1..=u64::MAX - 1][..]);
    }
}
