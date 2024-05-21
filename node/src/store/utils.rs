use std::ops::RangeInclusive;
use std::iter::once;

use itertools::Itertools;

use crate::store::{HeaderRanges, Store, StoreError};

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

/*
fn try_find_range_for_height<R>(
    ranges_table: &R,
    new_range: HeaderRange,
) -> Result<RangeScanInformation>
where
    R: ReadableTable<u64, (u64, u64)>,
{
    // ranges are kept in ascending order, since usually we're inserting at the
    // front
    let range_iter = ranges_table.iter()?.rev();

    let mut found_range: Option<RangeScanInformation> = None;
    let mut head_range = true;

    for range in range_iter {
        let (key, bounds) = range?;
        let (start, end) = bounds.value();

        println!("considering {start}..={end}");

        // appending to the found range
        if end + 1 == *new_range.start() {
            if let Some(previously_found_range) = found_range.as_mut() {
                previously_found_range.range_to_remove = Some(key.value());
                previously_found_range.range = start..=*previously_found_range.range.end();
                previously_found_range.neighbours_exist = (true, true);
            } else {
                found_range = Some(RangeScanInformation {
                    range_index: key.value(),
                    range: start..=*new_range.end(),
                    range_to_remove: None,
                    neighbours_exist: (true, false),
                });
            }
        }

        // we return only after considering range after and before provided height
        // to have the opportunity to consolidate existing ranges into one
        if let Some(found_range) = found_range {
            return Ok(found_range);
        }

        // prepending to the found range
        if start - 1 == *new_range.end() {
            found_range = Some(RangeScanInformation {
                range_index: key.value(),
                range: *new_range.start()..=end,
                range_to_remove: None,
                neighbours_exist: (false, true),
            });
        }

        println!("{} > {}", *new_range.start(), end);

        // XXX: ught clone
        if let Some(intersection) = ranges_intersection(new_range.clone(), start..=end) {
            return Err(StoreError::HeaderRangeOverlap(*intersection.start(), *intersection.end()));
        }
        // if new range is in front of or overlaping considered range
        if *new_range.end() > end && found_range.is_none() {
            // allow creation of a new range in front of the head range
            if head_range {
                return Ok(RangeScanInformation {
                    range_index: key.value() + 1,
                    range: new_range,
                    range_to_remove: None,
                    neighbours_exist: (false, false),
                });
            } else {
                tracing::error!("intra range error for height {new_range:?}: {start}..={end}");
                return Err(StoreError::HeaderRangeOverlap(end, *new_range.start()));
            }
        }

        //if *new_range.end() > end 

        /*
         * XXX: shouldn't be necessary
        if (start..=end).contains(&height) {
            return Err(StoreError::HeightExists(height));
        }
        */

        // only allow creating new ranges in front of the highest range
        head_range = false;
    }

    // return in case we're prepending and there only one range, thus one iteration
    if let Some(found_range) = found_range {
        return Ok(found_range);
    }

    // allow creation of new range at any height for an empty store
    if head_range {
        Ok(RangeScanInformation {
            range_index: 0,
            range: new_range,
            range_to_remove: None,
            neighbours_exist: (false, false),
        })
    } else {
        // TODO: errors again
        Err(StoreError::InsertPlacementDisallowed(*new_range.start(), *new_range.end()))
    }
}
*/


pub(crate) fn ranges_intersection(left: RangeInclusive<u64>, right: RangeInclusive<u64>) -> Option<RangeInclusive<u64>> {
    debug_assert!(left.start() <= left.end());
    debug_assert!(right.start() <= right.end());

    println!("intersecting {left:?} {right:?}");
    if dbg!(left.start() > right.end()) || dbg!(left.end() < right.start()) {
        println!("nop");
        return None;
    }

    match dbg!((left.start() >= right.start(), left.end() >= right.end())) {
        (false, false) => Some(*right.start()..=*left.end()),
        (false, true) => Some(right),
        (true, false) => Some(left),
        (true, true) => Some(*left.start()..=*right.end())
    }
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
        assert_eq!(ranges_intersection(1..=2, 3..=4), None);
        assert_eq!(ranges_intersection(1..=2, 6..=9), None);
        assert_eq!(ranges_intersection(3..=8, 1..=2), None);
        assert_eq!(ranges_intersection(1..=2, 4..=6), None);
    }

    #[test]
    fn intersection_overlapping() {
        assert_eq!(ranges_intersection(1..=2, 2..=4), Some(2..=2));
        assert_eq!(ranges_intersection(1..=2, 2..=2), Some(2..=2));
        assert_eq!(ranges_intersection(1..=5, 2..=9), Some(2..=5));
        assert_eq!(ranges_intersection(4..=6, 1..=9), Some(4..=6));
        assert_eq!(ranges_intersection(3..=7, 5..=5), Some(5..=5));
        assert_eq!(ranges_intersection(3..=7, 5..=6), Some(5..=6));
        assert_eq!(ranges_intersection(3..=5, 3..=3), Some(3..=3));
        assert_eq!(ranges_intersection(3..=5, 1..=4), Some(3..=4));
    }
}
