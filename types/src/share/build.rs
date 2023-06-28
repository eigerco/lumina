use std::io::Cursor;

use crate::consts::appconsts;
use crate::nmt::Namespace;
use crate::{Error, Result};
use crate::{InfoByte, Share};
use bytes::{Buf, BufMut, BytesMut};

// celestia-app/pkg/shares/share_builder
// NOTE: this is a simplification of the Go code above that doesn't involve
// manually calling it's different methods in correct order. It serves good
// now, but we may need to create a real Builder struct in a future when
// more flexibility will be needed.
impl Share {
    /// Builds a `Share` from info and data.
    ///
    /// Returns a `Share` and the leftover data if any.
    pub(crate) fn build(
        namespace: Namespace,
        share_version: u8,
        data: &mut Cursor<impl AsRef<[u8]>>,
    ) -> Result<Self> {
        let is_first_share = data.position() == 0;
        let data_len = cursor_inner_length(data);
        let mut bytes = BytesMut::with_capacity(appconsts::SHARE_SIZE);

        // Write the namespace
        bytes.put_slice(namespace.as_bytes());
        // Write the info byte
        let info_byte = InfoByte::new(share_version, is_first_share)?;
        bytes.put_u8(info_byte.as_u8());

        // If this share is first in the sequence, write the bytes len of the sequence
        if is_first_share {
            bytes.put_u32(
                data_len
                    .try_into()
                    .map_err(|_| Error::ShareSequenceLenExceeded(data_len))?,
            );
        }

        // If the share is compact, write the index of the next unit
        let is_compact_share = is_compact_share(&namespace);
        if is_compact_share {
            todo!("Compact shares are not supported yet");
        }

        let current_size = bytes.len();
        let available_space = appconsts::SHARE_SIZE - current_size;
        let read_amount = available_space.min(data.remaining());

        bytes.resize(current_size + read_amount, 0);
        data.copy_to_slice(&mut bytes[current_size..]);

        // Pad with zeroes if necessary
        if data.position() == data_len as u64 && !is_compact_share {
            bytes.resize(appconsts::SHARE_SIZE, 0);
        }

        Share::new(bytes.to_vec())
    }
}

fn is_compact_share(ns: &Namespace) -> bool {
    ns.is_tx() || ns.is_pay_for_blob()
}

fn cursor_inner_length(cursor: &Cursor<impl AsRef<[u8]>>) -> usize {
    cursor.get_ref().as_ref().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_sparse_share() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let share_version = appconsts::SHARE_VERSION_ZERO;
        let data = vec![1, 2, 3, 4, 5, 6, 7];
        let mut cursor = Cursor::new(&data);
        let expected_share_start: &[u8] = &[
            1, // info byte
            0, 0, 0, 7, // sequence len
            1, 2, 3, 4, 5, 6, 7, // data
        ];

        let share = Share::build(namespace.clone(), share_version, &mut cursor).unwrap();

        assert_eq!(cursor.position(), data.len() as u64);
        assert_eq!(namespace, share.namespace);

        // check data
        assert_eq!(
            &share.data[..expected_share_start.len()],
            expected_share_start
        );
        // check padding
        assert_eq!(
            &share.data[expected_share_start.len()..],
            &vec![0; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE - data.len()],
        );
    }

    #[test]
    fn test_sparse_share_with_continuation() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let continuation_len = 7;
        let share_version = appconsts::SHARE_VERSION_ZERO;
        let data = vec![7; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE + continuation_len];
        let mut cursor = Cursor::new(&data);

        let first_share = Share::build(namespace.clone(), share_version, &mut cursor).unwrap();

        assert_eq!(
            cursor.position(),
            appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE as u64
        );
        assert_eq!(namespace, first_share.namespace);
        // check sequence len
        let sequence_len = &first_share.data[appconsts::SHARE_INFO_BYTES
            ..appconsts::SHARE_INFO_BYTES + appconsts::SEQUENCE_LEN_BYTES];
        let sequence_len = u32::from_be_bytes(sequence_len.try_into().unwrap());
        assert_eq!(sequence_len, data.len() as u32);
        // check data
        assert_eq!(
            &first_share.data[appconsts::SHARE_INFO_BYTES + appconsts::SEQUENCE_LEN_BYTES..],
            &vec![7; appconsts::FIRST_SPARSE_SHARE_CONTENT_SIZE]
        );

        // Continuation share
        let expected_continuation_share_start: &[u8] = &[
            0, // info byte
            7, 7, 7, 7, 7, 7, 7, // data
        ];
        let continuation_share =
            Share::build(namespace.clone(), share_version, &mut cursor).unwrap();

        assert_eq!(cursor.position(), data.len() as u64);
        assert_eq!(namespace, continuation_share.namespace);

        // check data
        assert_eq!(
            &continuation_share.data[..expected_continuation_share_start.len()],
            expected_continuation_share_start
        );
        // check padding
        assert_eq!(
            &continuation_share.data[expected_continuation_share_start.len()..],
            &vec![
                0;
                appconsts::CONTINUATION_SPARSE_SHARE_CONTENT_SIZE
                    - expected_continuation_share_start.len()
                    + appconsts::SHARE_INFO_BYTES
            ],
        );
    }
}
