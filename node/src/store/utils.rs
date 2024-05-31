use std::fmt::Display;
use std::iter::once;
use std::ops::RangeInclusive;

use celestia_types::ExtendedHeader;
use itertools::Itertools;
use serde::Serialize;
use smallvec::{IntoIter, SmallVec};
use thiserror::Error;

use crate::executor::yield_now;
use crate::store::{Result, StoreError};

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

pub type HeaderRange = RangeInclusive<u64>;

pub(crate) trait RangeLengthExt {
    fn len(&self) -> u64;
}

impl RangeLengthExt for RangeInclusive<u64> {
    fn len(&self) -> u64 {
        self.end() - self.start() + 1
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct HeaderRanges(SmallVec<[RangeInclusive<u64>; 2]>);

#[derive(Debug, Error)]
pub enum HeaderRangeError {
    #[error("Overlapping range: {0}, {1}")]
    RangeOverlap(u64, u64),
}

impl HeaderRanges {
    pub fn validate(&self) -> Result<(), HeaderRangeError> {
        let mut prev: Option<&RangeInclusive<u64>> = None;
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

    /// Commit previously calculated `check_range_insert`
    pub(crate) fn update_range(&mut self, scan_information: RangeScanResult) {
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

    /// Return whether range is empty
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|r| r.is_empty())
    }

    pub fn contains(&self, height: u64) -> bool {
        self.0.iter().any(|r| r.contains(&height))
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
                write!(f, "{}..={}", range.start(), range.end())?;
            } else {
                write!(f, ", {}..={}", range.start(), range.end())?;
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
    inner_iter: Option<RangeInclusive<u64>>,
    outer_iter: IntoIter<[RangeInclusive<u64>; 2]>,
}

impl HeaderRangesIterator {
    pub fn next_batch(&mut self, limit: u64) -> Option<RangeInclusive<u64>> {
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
        if let Some(v) = self.inner_iter.as_mut()?.next() {
            return Some(v);
        }
        self.inner_iter = self.outer_iter.next();
        self.next()
    }
}

/// based on the stored headers and current network head height, calculate range of headers that
/// should be fetched from the network, starting from the front up to a `limit` of headers
pub(crate) fn calculate_missing_ranges(
    head_height: u64,
    store_headers: &[RangeInclusive<u64>],
    limit: u64,
) -> HeaderRanges {
    let range_edges = once(head_height + 1)
        .chain(
            store_headers
                .iter()
                .flat_map(|r| [*r.start(), *r.end()])
                .rev(),
        )
        .chain(once(0));

    let mut left = limit;
    let missing_ranges = range_edges
        .chunks(2)
        .into_iter()
        .map_while(|mut range| {
            if left == 0 {
                return None;
            }
            // ranges_edges is even and we divide into 2 element chunks, this is safe
            let upper_bound = range.next().unwrap() - 1;
            let lower_bound = range.next().unwrap();
            let range_len = upper_bound.checked_sub(lower_bound)?;

            if range_len == 0 {
                // empty range
                return Some(RangeInclusive::new(1, 0));
            }

            if left > range_len {
                left -= range_len;
                Some(lower_bound + 1..=upper_bound)
            } else {
                let truncated_lower_bound = upper_bound - left;
                left = 0;
                Some(truncated_lower_bound + 1..=upper_bound)
            }
        })
        .filter(|v| !v.is_empty())
        .collect();
    HeaderRanges(missing_ranges)
}

pub(crate) fn try_consolidate_ranges(
    left: &RangeInclusive<u64>,
    right: &RangeInclusive<u64>,
) -> Option<RangeInclusive<u64>> {
    debug_assert!(left.start() <= left.end());
    debug_assert!(right.start() <= right.end());

    if left.end() + 1 == *right.start() {
        return Some(*left.start()..=*right.end());
    }

    if right.end() + 1 == *left.start() {
        return Some(*right.start()..=*left.end());
    }

    None
}

pub(crate) fn ranges_intersection(
    left: &RangeInclusive<u64>,
    right: &RangeInclusive<u64>,
) -> Option<RangeInclusive<u64>> {
    debug_assert!(left.start() <= left.end());
    debug_assert!(right.start() <= right.end());

    if left.start() > right.end() || left.end() < right.start() {
        return None;
    }

    match (left.start() >= right.start(), left.end() >= right.end()) {
        (false, false) => Some(*right.start()..=*left.end()),
        (false, true) => Some(right.clone()),
        (true, false) => Some(left.clone()),
        (true, true) => Some(*left.start()..=*right.end()),
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct RangeScanResult {
    /// index of the range that header is being inserted into
    pub range_index: usize,
    /// updated bounds of the range header is being inserted into
    pub range: HeaderRange,
    /// index of the range that should be removed from the table, if we're consolidating two
    /// ranges. None otherwise.
    pub range_to_remove: Option<usize>,
}

pub(crate) fn check_range_insert(
    stored_ranges: &HeaderRanges,
    to_insert: &RangeInclusive<u64>,
) -> Result<RangeScanResult> {
    let Some(head_range) = stored_ranges.0.last() else {
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
            range_index: stored_ranges.0.len(),
            range: to_insert.clone(),
            range_to_remove: None,
        });
    }

    let mut stored_ranges_iter = stored_ranges.0.iter().enumerate();
    let mut found_range = loop {
        let Some((idx, stored_range)) = stored_ranges_iter.next() else {
            return Err(StoreError::InsertPlacementDisallowed(
                *to_insert.start(),
                *to_insert.end(),
            ));
        };

        // TODO: break early
        //if sth

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

pub(crate) fn verify_range_contiguous(headers: &[ExtendedHeader]) -> Result<()> {
    let mut prev = None;
    for h in headers {
        let current_height = h.height().value();
        if let Some(prev_height) = prev {
            if prev_height + 1 != current_height {
                return Err(StoreError::InsertRangeWithGap(prev_height, current_height));
            }
        }
        prev = Some(current_height);
    }
    Ok(())
}

#[allow(unused)]
pub(crate) async fn validate_headers(headers: &[ExtendedHeader]) -> celestia_types::Result<()> {
    for headers in headers.chunks(VALIDATIONS_PER_YIELD) {
        for header in headers {
            header.validate()?;
        }

        // Validation is computation heavy so we yield on every chunk
        yield_now().await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_missing_ranges() {
        let head_height = 50;
        let ranges = [
            RangeInclusive::new(1, 5),
            RangeInclusive::new(15, 20),
            RangeInclusive::new(23, 28),
            RangeInclusive::new(30, 40),
        ];
        let expected_missing_ranges = [41..=50, 29..=29, 21..=22, 6..=14].into();

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 512);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_partial() {
        let head_height = 10;
        let ranges = [RangeInclusive::new(6, 7)];
        let expected_missing_ranges = [8..=10, 4..=5].into();

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 5);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_contiguous() {
        let head_height = 10;
        let ranges = [RangeInclusive::new(5, 6), RangeInclusive::new(7, 9)];
        let expected_missing_ranges = [10..=10, 1..=4].into();

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 5);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_edge_cases() {
        let missing = calculate_missing_ranges(1, &[], 100);
        assert_eq!(missing, [1..=1].into());

        let missing = calculate_missing_ranges(1, &[1..=1], 100);
        assert_eq!(missing, [].into());
    }

    #[test]
    fn intersection_non_overlapping() {
        assert_eq!(ranges_intersection(&(1..=2), &(3..=4)), None);
        assert_eq!(ranges_intersection(&(1..=2), &(6..=9)), None);
        assert_eq!(ranges_intersection(&(3..=8), &(1..=2)), None);
        assert_eq!(ranges_intersection(&(1..=2), &(4..=6)), None);
    }

    #[test]
    fn intersection_overlapping() {
        assert_eq!(ranges_intersection(&(1..=2), &(2..=4)), Some(2..=2));
        assert_eq!(ranges_intersection(&(1..=2), &(2..=2)), Some(2..=2));
        assert_eq!(ranges_intersection(&(1..=5), &(2..=9)), Some(2..=5));
        assert_eq!(ranges_intersection(&(4..=6), &(1..=9)), Some(4..=6));
        assert_eq!(ranges_intersection(&(3..=7), &(5..=5)), Some(5..=5));
        assert_eq!(ranges_intersection(&(3..=7), &(5..=6)), Some(5..=6));
        assert_eq!(ranges_intersection(&(3..=5), &(3..=3)), Some(3..=3));
        assert_eq!(ranges_intersection(&(3..=5), &(1..=4)), Some(3..=4));
    }

    #[test]
    fn check_range_insert_append() {
        let result = check_range_insert(&[].into(), &(1..=5)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(&[1..=4].into(), &(5..=5)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(&[1..=5].into(), &(6..=9)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(&[6..=8].into(), &(2..=5)).unwrap();
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
        let result = check_range_insert(&[1..=3, 6..=9].into(), &(4..=5)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: Some(1),
            }
        );

        let result = check_range_insert(&[1..=2, 5..=5, 8..=9].into(), &(3..=4)).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: Some(1),
            }
        );

        let result = check_range_insert(&[1..=2, 4..=4, 8..=9].into(), &(5..=7)).unwrap();
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
        let result = check_range_insert(&[1..=2].into(), &(1..=1)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(1, 1)));

        let result = check_range_insert(&[1..=4].into(), &(2..=8)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 4)));

        let result = check_range_insert(&[1..=4].into(), &(2..=3)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(2, 3)));

        let result = check_range_insert(&[5..=9].into(), &(1..=5)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 5)));

        let result = check_range_insert(&[5..=8].into(), &(2..=8)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(5, 8)));

        let result = check_range_insert(&[1..=3, 6..=9].into(), &(3..=6)).unwrap_err();
        assert!(matches!(result, StoreError::HeaderRangeOverlap(3, 3)));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = check_range_insert(&[1..=2, 7..=9].into(), &(4..=4)).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = check_range_insert(&[1..=2, 8..=9].into(), &(4..=6)).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = check_range_insert(&[4..=5, 7..=8].into(), &(1..=2)).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));
    }
}
