use std::iter::once;
use std::ops::RangeInclusive;

use itertools::Itertools;

use crate::store::{HeaderRange, HeaderRanges, Result, StoreError};

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
                .inspect(|r| println!("{r:?}"))
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

    match dbg!((left.start() >= right.start(), left.end() >= right.end())) {
        (false, false) => Some(*right.start()..=*left.end()),
        (false, true) => Some(right.clone()),
        (true, false) => Some(left.clone()),
        (true, true) => Some(*left.start()..=*right.end()),
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct RangeScanResult {
    /// index of the range that header is being inserted into
    pub range_index: u64,
    /// updated bounds of the range header is being inserted into
    pub range: HeaderRange,
    /// index of the range that should be removed from the table, if we're consolidating two
    /// ranges. None otherwise.
    pub range_to_remove: Option<u64>,
    // cached information about whether previous and next header exist in store
    //neighbours_exist: (bool, bool),
}

pub(crate) fn check_range_insert(
    stored_ranges: HeaderRanges,
    to_insert: RangeInclusive<u64>,
) -> Result<RangeScanResult> {
    // TODO: don't require reverse

    let Some(head_range) = stored_ranges.0.last() else {
        // Empty store case
        return Ok(RangeScanResult {
            range_index: 0,
            range: to_insert,
            range_to_remove: None,
            //neighbours_exist: (false, false)
        });
    };

    // allow inserting a new header range in front of the current head range
    if *to_insert.start() > head_range.end() + 1 {
        println!("pre ppp: {to_insert:?} > {head_range:?}");
        return Ok(RangeScanResult {
            // XXX: should db key be usize?
            range_index: stored_ranges.0.len() as u64,
            range: to_insert,
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

        if let Some(intersection) = ranges_intersection(stored_range, &to_insert) {
            return Err(StoreError::HeaderRangeOverlap(
                *intersection.start(),
                *intersection.end(),
            ));
        }

        if let Some(consolidated) = try_consolidate_ranges(stored_range, &to_insert) {
            break RangeScanResult {
                range_index: idx as u64,
                range: consolidated,
                range_to_remove: None,
            };
        }
    };

    // we have a hit, check whether we can merge with the next range too
    if let Some((idx, range_after)) = stored_ranges_iter.next() {
        if let Some(intersection) = ranges_intersection(range_after, &to_insert) {
            return Err(StoreError::HeaderRangeOverlap(
                *intersection.start(),
                *intersection.end(),
            ));
        }

        if let Some(consolidated) = try_consolidate_ranges(range_after, &found_range.range) {
            found_range = RangeScanResult {
                range_index: found_range.range_index,
                range: consolidated,
                range_to_remove: Some(idx as u64),
            };
        }
    }

    Ok(found_range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn test_calc_missing_ranges() {
        let head_height = 50;
        let ranges = [
            RangeInclusive::new(1, 5),
            RangeInclusive::new(15, 20),
            RangeInclusive::new(23, 28),
            RangeInclusive::new(30, 40),
        ];
        let expected_missing_ranges = HeaderRanges(smallvec![41..=50, 29..=29, 21..=22, 6..=14,]);

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 512);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_partial() {
        let head_height = 10;
        let ranges = [RangeInclusive::new(6, 7)];
        let expected_missing_ranges = HeaderRanges(smallvec![8..=10, 4..=5]);

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 5);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_contiguous() {
        let head_height = 10;
        let ranges = [RangeInclusive::new(5, 6), RangeInclusive::new(7, 9)];
        let expected_missing_ranges = HeaderRanges(smallvec![10..=10, 1..=4]);

        let missing_ranges = calculate_missing_ranges(head_height, &ranges, 5);
        assert_eq!(missing_ranges, expected_missing_ranges);
    }

    #[test]
    fn test_calc_missing_ranges_edge_cases() {
        let missing = calculate_missing_ranges(1, &[], 100);
        assert_eq!(missing, HeaderRanges(smallvec![1..=1]));

        let missing = calculate_missing_ranges(1, &[1..=1], 100);
        assert_eq!(missing, HeaderRanges(smallvec![]));
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
        let result = check_range_insert(HeaderRanges(smallvec![]), 1..=5).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(HeaderRanges(smallvec![1..=4]), 5..=5).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(HeaderRanges(smallvec![1..=5]), 6..=9).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: None,
            }
        );

        let result = check_range_insert(HeaderRanges(smallvec![6..=8]), 2..=5).unwrap();
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
        let result = check_range_insert(HeaderRanges(smallvec![1..=3, 6..=9]), 4..=5).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=9,
                range_to_remove: Some(1),
            }
        );

        let result = check_range_insert(HeaderRanges(smallvec![1..=2, 5..=5, 8..=9]), 3..=4).unwrap();
        assert_eq!(
            result,
            RangeScanResult {
                range_index: 0,
                range: 1..=5,
                range_to_remove: Some(1),
            }
        );

        let result = check_range_insert(HeaderRanges(smallvec![1..=2, 4..=4, 8..=9]), 5..=7).unwrap();
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
        let result = check_range_insert(HeaderRanges(smallvec![1..=2]), 1..=1).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(1, 1)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![1..=4]), 2..=8).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(2, 4)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![1..=4]), 2..=3).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(2, 3)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![5..=9]), 1..=5).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(5, 5)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![5..=8]), 2..=8).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(5, 8)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![1..=3, 6..=9]), 3..=6).unwrap_err();
        assert!(matches!(
            result,
            StoreError::HeaderRangeOverlap(3, 3)
        ));
    }

    #[test]
    fn check_range_insert_invalid_placement() {
        let result = check_range_insert(HeaderRanges(smallvec![1..=2, 7..=9]), 4..=4).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 4)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![1..=2, 8..=9]), 4..=6).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(4, 6)
        ));

        let result = check_range_insert(HeaderRanges(smallvec![4..=5, 7..=8]), 1..=2).unwrap_err();
        assert!(matches!(
            result,
            StoreError::InsertPlacementDisallowed(1, 2)
        ));
    }
}
