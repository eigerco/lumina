use std::num::NonZeroU64;

use tendermint::{crypto, merkle};
use tendermint_proto::v0_34::types::Blob as RawBlob;

use crate::consts::appconsts;
use crate::nmt::Namespace;
use crate::{Error, Result, Share};

/// create_commitment generates the share commitment for a given blob.
/// See [Message layout rationale] and [Non-interactive default rules].
///
/// [Message layout rationale]: https://github.com/celestiaorg/celestia-specs/blob/e59efd63a2165866584833e91e1cb8a6ed8c8203/src/rationale/message_block_layout.md?plain=1#L12
/// [Non-interactive default rules]: https://github.com/celestiaorg/celestia-specs/blob/e59efd63a2165866584833e91e1cb8a6ed8c8203/src/rationale/message_block_layout.md?plain=1#L36
pub fn create_commitment(blob: &RawBlob) -> Result<Vec<u8>> {
    let namespace = Namespace::new(blob.namespace_version as u8, &blob.namespace_id)?;
    // TODO: we could probably lower amount of allocations there if
    // Shares were just a wrapper over a raw data
    let shares = split_blob_to_shares(blob)?;

    // the commitment is the root of a merkle mountain range with max tree size
    // determined by the number of roots required to create a share commitment
    // over that blob. The size of the tree is only increased if the number of
    // subtree roots surpasses a constant threshold.
    let subtree_width = subtree_width(shares.len() as u64, appconsts::SUBTREE_ROOT_THRESHOLD);
    let tree_sizes = merkle_mountain_range_sizes(shares.len() as u64, subtree_width);

    let mut shares = shares;
    let mut leaf_sets: Vec<Vec<Share>> = Vec::new();

    for size in tree_sizes {
        let left_shares = shares.split_off(size as usize);
        leaf_sets.push(shares);
        shares = left_shares;
    }

    // create the commitments by pushing each leaf set onto an nmt
    let mut subtree_roots: Vec<Vec<u8>> = Vec::new();
    for leaf_set in leaf_sets {
        // create the nmt
        let mut tree = crate::nmt::Nmt::new();
        for leaf_share in leaf_set {
            tree.push_leaf(
                &leaf_share.to_vec(),
                nmt_rs::NamespaceId(namespace.as_bytes().try_into().unwrap()),
            )
            .map_err(Error::Nmt)?;
        }
        // add the root
        subtree_roots.push(hash_to_bytes(tree.root()));
    }

    Ok(merkle::simple_hash_from_byte_vectors::<crypto::default::Sha256>(&subtree_roots).to_vec())
}

fn split_blob_to_shares(blob: &RawBlob) -> Result<Vec<Share>> {
    let namespace = Namespace::new(blob.namespace_version as u8, &blob.namespace_id)?;
    let mut shares = Vec::new();
    let mut data = blob.data.clone();

    while !data.is_empty() {
        let (share, leftover) = Share::build(
            namespace.clone(),
            appconsts::SHARE_VERSION_ZERO,
            shares.is_empty(),
            data,
        )?;
        shares.push(share);
        data = if let Some(leftover) = leftover {
            leftover
        } else {
            Vec::with_capacity(0)
        };
    }
    Ok(shares)
}

/// merkle_mountain_range_sizes returns the sizes (number of leaf nodes) of the
/// trees in a merkle mountain range constructed for a given total_size and
/// max_tree_size.
///
/// https://docs.grin.mw/wiki/chain-state/merkle-mountain-range/
/// https://github.com/opentimestamps/opentimestamps-server/blob/master/doc/merkle-mountain-range.md
fn merkle_mountain_range_sizes(mut total_size: u64, max_tree_size: u64) -> Vec<u64> {
    let mut tree_sizes = Vec::new();

    while total_size != 0 {
        if total_size >= max_tree_size {
            tree_sizes.push(max_tree_size);
            total_size -= max_tree_size;
        } else {
            let tree_size = round_down_to_power_of_2(
                // unwrap is safe as total_size can't be zero there
                total_size.try_into().unwrap(),
            )
            .expect("Failed to find next power of 2");
            tree_sizes.push(tree_size);
            total_size -= tree_size;
        }
    }

    tree_sizes
}

fn hash_to_bytes<const NS_SIZE: usize>(hash: nmt_rs::NamespacedHash<NS_SIZE>) -> Vec<u8> {
    hash.iter().copied().collect()
}

/// blob_min_square_size returns the minimum square size that can contain share_count
/// number of shares.
fn blob_min_square_size(share_count: u64) -> u64 {
    round_up_to_power_of_2((share_count as f64).sqrt().ceil() as u64)
        .expect("Failed to find minimum blob square size")
}

/// subtree_width determines the maximum number of leaves per subtree in the share
/// commitment over a given blob. The input should be the total number of shares
/// used by that blob. The reasoning behind this algorithm is discussed in depth
/// in ADR013
/// (celestia-app/docs/architecture/adr-013-non-interative-default-rules-for-zero-padding).
fn subtree_width(share_count: u64, subtree_root_threshold: u64) -> u64 {
    // per ADR013, we use a predetermined threshold to determine width of sub
    // trees used to create share commitments
    let mut s = share_count / subtree_root_threshold;

    // round up if the width is not an exact multiple of the threshold
    if share_count % subtree_root_threshold != 0 {
        s += 1;
    }

    // use a power of two equal to or larger than the multiple of the subtree
    // root threshold
    s = round_up_to_power_of_2(s).expect("Failed to find next power of 2");

    // use the minimum of the subtree width and the min square size, this
    // gurarantees that a valid value is returned
    // return min(s, BlobMinSquareSize(shareCount))
    s.min(blob_min_square_size(share_count))
}

/// round_up_to_power_of_2 returns the next power of two that is strictly greater than input.
fn round_up_to_power_of_2(x: u64) -> Option<u64> {
    let mut po2 = 1;

    loop {
        if po2 >= x {
            return Some(po2);
        }
        if let Some(next_po2) = po2.checked_shl(1) {
            po2 = next_po2;
        } else {
            return None;
        }
    }
}

/// round_down_to_power_of_2 returns the next power of two less than or equal to input.
fn round_down_to_power_of_2(x: NonZeroU64) -> Option<u64> {
    let x: u64 = x.into();

    match round_up_to_power_of_2(x) {
        Some(po2) if po2 == x => Some(x),
        Some(po2) => Some(po2 / 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merkle_mountain_ranges() {
        struct TestCase {
            total_size: u64,
            square_size: u64,
            expected: Vec<u64>,
        }

        let test_cases = [
            TestCase {
                total_size: 11,
                square_size: 4,
                expected: vec![4, 4, 2, 1],
            },
            TestCase {
                total_size: 2,
                square_size: 64,
                expected: vec![2],
            },
            TestCase {
                total_size: 64,
                square_size: 8,
                expected: vec![8, 8, 8, 8, 8, 8, 8, 8],
            },
            // Height
            // 3              x                               x
            //              /    \                         /    \
            //             /      \                       /      \
            //            /        \                     /        \
            //           /          \                   /          \
            // 2        x            x                 x            x
            //        /   \        /   \             /   \        /   \
            // 1     x     x      x     x           x     x      x     x         x
            //      / \   / \    / \   / \         / \   / \    / \   / \      /   \
            // 0   0   1 2   3  4   5 6   7       8   9 10  11 12 13 14  15   16   17    18
            TestCase {
                total_size: 19,
                square_size: 8,
                expected: vec![8, 8, 2, 1],
            },
        ];
        for case in test_cases {
            assert_eq!(
                merkle_mountain_range_sizes(case.total_size, case.square_size),
                case.expected,
            );
        }
    }
}
