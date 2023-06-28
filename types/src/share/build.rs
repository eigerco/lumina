use crate::consts::appconsts;
use crate::nmt::Namespace;
use crate::{Error, Result};
use crate::{InfoByte, Share};
use bytes::{BufMut, BytesMut};

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
        is_first_share: bool,
        mut data: Vec<u8>,
    ) -> Result<(Self, Option<Vec<u8>>)> {
        let mut bytes = BytesMut::with_capacity(appconsts::SHARE_SIZE).limit(appconsts::SHARE_SIZE);

        // Write the namespace
        bytes.put_slice(namespace.as_bytes());
        // Write the info byte
        let info_byte = InfoByte::new(share_version, is_first_share)?;
        bytes.put_u8(info_byte.as_u8());

        // If this share is first in the sequence, write the bytes len of the sequence
        if is_first_share {
            bytes.put_u32(
                data.len()
                    .try_into()
                    .map_err(|_| Error::ShareSequenceLenExceeded(data.len()))?,
            );
        }

        // If the share is compact, write the index of the next unit
        let is_compact_share = is_compact_share(&namespace);
        if is_compact_share {
            todo!("Compact shares are not supported yet");
        }

        // Check if we can write whole data or only a part of it
        let leftover = if data.len() > bytes.remaining_mut() {
            Some(data.split_off(bytes.remaining_mut()))
        } else {
            None
        };

        // Write the actual data
        bytes.put(&data[..]);

        // Pad with zeroes if necessary
        if leftover.is_none() && !is_compact_share {
            bytes.get_mut().resize(appconsts::SHARE_SIZE, 0);
        }

        Ok((Share::new(bytes.into_inner().to_vec())?, leftover))
    }
}

fn is_compact_share(ns: &Namespace) -> bool {
    ns.is_tx() || ns.is_pay_for_blob()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_sparse_share() {
        let namespace = Namespace::new(0, &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]).unwrap();
        let share_version = appconsts::SHARE_VERSION_ZERO;
        let data = vec![1, 2, 3, 4, 5, 6, 7];
        let expected_share_start: &[u8] = &[
            1, // info byte
            0, 0, 0, 7, // sequence len
            1, 2, 3, 4, 5, 6, 7, // data
        ];

        let (share, leftover) =
            Share::build(namespace.clone(), share_version, true, data.clone()).unwrap();

        assert!(leftover.is_none());
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

        let (first_share, leftover) =
            Share::build(namespace.clone(), share_version, true, data.clone()).unwrap();

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
        let (continuation_share, leftover) =
            Share::build(namespace.clone(), share_version, false, leftover.unwrap()).unwrap();

        assert!(leftover.is_none());
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
