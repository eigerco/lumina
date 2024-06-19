use std::ops::RangeInclusive;

use celestia_types::ExtendedHeader;

use crate::executor::yield_now;
use crate::store::header_ranges::HeaderRange;
use crate::store::{Result, StoreError};

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

/// based on the stored headers and current network head height, calculate range of headers that
/// should be fetched from the network, starting from the front up to a `limit` of headers
pub(crate) fn calculate_range_to_fetch(
    head_height: u64,
    store_headers: &[RangeInclusive<u64>],
    limit: u64,
) -> HeaderRange {
    let mut store_headers_iter = store_headers.iter().rev();

    let Some(store_head_range) = store_headers_iter.next() else {
        // empty store, just fetch from head
        return head_height.saturating_sub(limit) + 1..=head_height;
    };

    if store_head_range.end() != &head_height {
        // if we haven't caught up with network head, start from there
        let fetch_start = u64::max(
            store_head_range.end() + 1,
            head_height.saturating_sub(limit) + 1,
        );
        return fetch_start..=head_height;
    }

    // there exists a range contiguous with network head. inspect previous range end
    let penultimate_range_end = store_headers_iter.next().map(|r| *r.end()).unwrap_or(0);

    let fetch_end = store_head_range.start().saturating_sub(1);
    let fetch_start = u64::max(
        penultimate_range_end + 1,
        fetch_end.saturating_sub(limit) + 1,
    );

    fetch_start..=fetch_end
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

#[allow(unused)]
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
    fn calculate_range_to_fetch_test_header_limit() {
        let head_height = 1024;
        let ranges = [256..=512];

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, 16);
        assert_eq!(fetch_range, 1009..=1024);

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, 511);
        assert_eq!(fetch_range, 514..=1024);
        let fetch_range = calculate_range_to_fetch(head_height, &ranges, 512);
        assert_eq!(fetch_range, 513..=1024);
        let fetch_range = calculate_range_to_fetch(head_height, &ranges, 513);
        assert_eq!(fetch_range, 513..=1024);

        let fetch_range = calculate_range_to_fetch(head_height, &ranges, 1024);
        assert_eq!(fetch_range, 513..=1024);
    }

    #[test]
    fn calculate_range_to_fetch_empty_store() {
        let fetch_range = calculate_range_to_fetch(1, &[], 100);
        assert_eq!(fetch_range, 1..=1);

        let fetch_range = calculate_range_to_fetch(100, &[], 10);
        assert_eq!(fetch_range, 91..=100);
    }

    #[test]
    fn calculate_range_to_fetch_fully_synced() {
        let fetch_range = calculate_range_to_fetch(1, &[1..=1], 100);
        assert!(fetch_range.is_empty());

        let fetch_range = calculate_range_to_fetch(100, &[1..=100], 10);
        assert!(fetch_range.is_empty());
    }

    #[test]
    fn calculate_range_to_fetch_caught_up() {
        let head_height = 4000;

        let fetch_range = calculate_range_to_fetch(head_height, &[3000..=4000], 500);
        assert_eq!(fetch_range, 2500..=2999);
        let fetch_range = calculate_range_to_fetch(head_height, &[500..=1000, 3000..=4000], 500);
        assert_eq!(fetch_range, 2500..=2999);
        let fetch_range = calculate_range_to_fetch(head_height, &[2500..=2800, 3000..=4000], 500);
        assert_eq!(fetch_range, 2801..=2999);
        let fetch_range = calculate_range_to_fetch(head_height, &[300..=4000], 500);
        assert_eq!(fetch_range, 1..=299);
    }

    #[test]
    fn calculate_range_to_fetch_catching_up() {
        let head_height = 4000;

        let fetch_range = calculate_range_to_fetch(head_height, &[2000..=3000], 500);
        assert_eq!(fetch_range, 3501..=4000);
        let fetch_range = calculate_range_to_fetch(head_height, &[1..=2998, 3000..=3800], 500);
        assert_eq!(fetch_range, 3801..=4000);
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
