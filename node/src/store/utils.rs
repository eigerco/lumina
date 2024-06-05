use std::iter::once;
use std::ops::RangeInclusive;

use celestia_types::ExtendedHeader;
use itertools::Itertools;

use crate::executor::yield_now;
use crate::store::{Result, StoreError};
use crate::store::header_ranges::{HeaderRange, HeaderRanges};

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

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
    range_edges
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
        .collect()
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
}
