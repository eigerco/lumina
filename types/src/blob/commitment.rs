use std::io::Cursor;
use std::num::NonZeroU64;

use crate::InfoByte;
use bytes::{Buf, BufMut, BytesMut};
use tendermint::{crypto, merkle};
use tendermint_proto::v0_34::types::Blob as RawBlob;

use crate::consts::appconsts;
use crate::nmt::{Namespace, Nmt};
use crate::{Error, Result};

type SparseShare = Vec<u8>;

/// create_commitment generates the share commitment for a given blob.
/// See [Message layout rationale] and [Non-interactive default rules].
///
/// [Message layout rationale]: https://github.com/celestiaorg/celestia-specs/blob/e59efd63a2165866584833e91e1cb8a6ed8c8203/src/rationale/message_block_layout.md?plain=1#L12
/// [Non-interactive default rules]: https://github.com/celestiaorg/celestia-specs/blob/e59efd63a2165866584833e91e1cb8a6ed8c8203/src/rationale/message_block_layout.md?plain=1#L36
pub fn create_commitment(blob: &RawBlob) -> Result<Vec<u8>> {
    let namespace = Namespace::new(blob.namespace_version as u8, &blob.namespace_id)?;
    let shares = split_blob_to_shares(blob)?;

    // the commitment is the root of a merkle mountain range with max tree size
    // determined by the number of roots required to create a share commitment
    // over that blob. The size of the tree is only increased if the number of
    // subtree roots surpasses a constant threshold.
    let subtree_width = subtree_width(shares.len() as u64, appconsts::SUBTREE_ROOT_THRESHOLD);
    let tree_sizes = merkle_mountain_range_sizes(shares.len() as u64, subtree_width);

    let mut shares = shares;
    let mut leaf_sets: Vec<Vec<SparseShare>> = Vec::new();

    for size in tree_sizes {
        let left_shares = shares.split_off(size as usize);
        leaf_sets.push(shares);
        shares = left_shares;
    }

    // create the commitments by pushing each leaf set onto an nmt
    let mut subtree_roots: Vec<Vec<u8>> = Vec::new();
    for leaf_set in leaf_sets {
        // create the nmt
        let mut tree = Nmt::new();
        for leaf_share in leaf_set {
            tree.push_leaf(&leaf_share, namespace.into())
                .map_err(Error::Nmt)?;
        }
        // add the root
        subtree_roots.push(hash_to_bytes(tree.root()));
    }

    Ok(merkle::simple_hash_from_byte_vectors::<crypto::default::Sha256>(&subtree_roots).to_vec())
}

/// Splits blob's data to the sequence of shares
fn split_blob_to_shares(blob: &RawBlob) -> Result<Vec<SparseShare>> {
    if blob.share_version != appconsts::SHARE_VERSION_ZERO as u32 {
        return Err(Error::UnsupportedShareVersion(blob.share_version));
    }

    let namespace = Namespace::new(blob.namespace_version as u8, &blob.namespace_id)?;
    let mut shares = Vec::new();
    let mut cursor = Cursor::new(&blob.data);

    while cursor.has_remaining() {
        let share = build_sparse_share_v0(namespace, &mut cursor)?;
        shares.push(share);
    }
    Ok(shares)
}

/// Build a sparse share from a cursor over data
fn build_sparse_share_v0(
    namespace: Namespace,
    data: &mut Cursor<impl AsRef<[u8]>>,
) -> Result<SparseShare> {
    let is_first_share = data.position() == 0;
    let data_len = cursor_inner_length(data);
    let mut bytes = BytesMut::with_capacity(appconsts::SHARE_SIZE);

    // Write the namespace
    bytes.put_slice(namespace.as_bytes());
    // Write the info byte
    let info_byte = InfoByte::new(appconsts::SHARE_VERSION_ZERO, is_first_share)?;
    bytes.put_u8(info_byte.as_u8());

    // If this share is first in the sequence, write the bytes len of the sequence
    if is_first_share {
        let data_len = data_len
            .try_into()
            .map_err(|_| Error::ShareSequenceLenExceeded(data_len))?;
        bytes.put_u32(data_len);
    }

    // Calculate amount of bytes to read
    let current_size = bytes.len();
    let available_space = appconsts::SHARE_SIZE - current_size;
    let read_amount = available_space.min(data.remaining());

    // Resize to share size with 0 padding
    bytes.resize(appconsts::SHARE_SIZE, 0);
    // Read the share data
    data.copy_to_slice(&mut bytes[current_size..current_size + read_amount]);

    Ok(bytes.to_vec())
}

fn cursor_inner_length(cursor: &Cursor<impl AsRef<[u8]>>) -> usize {
    cursor.get_ref().as_ref().len()
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
    fn test_single_sparse_share() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let data = vec![1, 2, 3, 4, 5, 6, 7];
        let mut cursor = Cursor::new(&data);

        let share = build_sparse_share_v0(namespace, &mut cursor).unwrap();

        // check cursor
        assert!(!cursor.has_remaining());

        // check namespace
        let (share_ns, share_data) = share.split_at(appconsts::NAMESPACE_SIZE);
        assert_eq!(share_ns, namespace.as_bytes());

        // check data
        let expected_share_start: &[u8] = &[
            1, // info byte
            0, 0, 0, 7, // sequence len
            1, 2, 3, 4, 5, 6, 7, // data
        ];
        let (share_data, share_padding) = share_data.split_at(expected_share_start.len());
        assert_eq!(share_data, expected_share_start);

        // check padding
        assert_eq!(
            share_padding,
            &vec![0; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE - data.len()],
        );
    }

    #[test]
    fn test_sparse_share_with_continuation() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let continuation_len = 7;
        let data = vec![7; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE + continuation_len];
        let mut cursor = Cursor::new(&data);

        let first_share = build_sparse_share_v0(namespace, &mut cursor).unwrap();

        // check cursor
        assert_eq!(
            cursor.position(),
            appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE as u64
        );

        // check namespace
        let (share_ns, share_data) = first_share.split_at(appconsts::NAMESPACE_SIZE);
        assert_eq!(share_ns, namespace.as_bytes());

        // check info byte
        let (share_info_byte, share_data) = share_data.split_at(appconsts::SHARE_INFO_BYTES);
        assert_eq!(share_info_byte, &[1]);

        // check sequence len
        let (share_seq_len, share_data) = share_data.split_at(appconsts::SEQUENCE_LEN_BYTES);
        assert_eq!(share_seq_len, &(data.len() as u32).to_be_bytes());

        // check data
        assert_eq!(
            share_data,
            &vec![7; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE]
        );

        // Continuation share
        let continuation_share = build_sparse_share_v0(namespace, &mut cursor).unwrap();

        // check cursor
        assert!(!cursor.has_remaining());

        // check namespace
        let (share_ns, share_data) = continuation_share.split_at(appconsts::NAMESPACE_SIZE);
        assert_eq!(share_ns, namespace.as_bytes());

        // check data
        let expected_continuation_share_start: &[u8] = &[
            0, // info byte
            7, 7, 7, 7, 7, 7, 7, // data
        ];
        let (share_data, share_padding) =
            share_data.split_at(expected_continuation_share_start.len());
        assert_eq!(share_data, expected_continuation_share_start);

        // check padding
        assert_eq!(
            share_padding,
            &vec![0; appconsts::CONTINUATION_SPARSE_SHARE_CONTENT_SIZE - continuation_len],
        );
    }

    #[test]
    fn test_sparse_share_empty_data() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let data = vec![];
        let mut cursor = Cursor::new(&data);
        let expected_share_start: &[u8] = &[
            1, // info byte
            0, 0, 0, 0, // sequence len
        ];

        let share = build_sparse_share_v0(namespace, &mut cursor).unwrap();

        // check cursor
        assert!(!cursor.has_remaining());

        // check namespace
        let (share_ns, share_data) = share.split_at(appconsts::NAMESPACE_SIZE);
        assert_eq!(share_ns, namespace.as_bytes());

        // check data
        let (share_start, share_data) = share_data.split_at(expected_share_start.len());
        assert_eq!(share_start, expected_share_start);

        // check padding
        assert_eq!(
            share_data,
            &vec![0; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE],
        );
    }

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
