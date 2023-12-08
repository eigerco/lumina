use std::io::Cursor;

use bytes::{Buf, BufMut, BytesMut};
use cid::CidGeneric;
use multihash::Multihash;
use sha2::{Digest, Sha256};

use crate::axis::AxisType;
use crate::multihash::{HasCid, HasMultihash};
use crate::nmt::{Namespace, NamespacedHashExt, HASH_SIZE, NS_SIZE};
use crate::{DataAvailabilityHeader, Error, Result};

const NAMESPACED_DATA_ID_SIZE: usize = NamespacedDataId::size();
pub const NAMESPACED_DATA_ID_MULTIHASH_CODE: u64 = 0x7821;
pub const NAMESPACED_DATA_ID_CODEC: u64 = 0x7820;

/// Represents shares from a namespace located on a particular row of Data Square
#[derive(Debug, PartialEq)]
pub struct NamespacedDataId {
    pub namespace: Namespace,
    pub row_index: u16,
    pub hash: [u8; HASH_SIZE],
    pub block_height: u64,
}

impl NamespacedDataId {
    /// Creates new NamespacedDataId for row and namespace, computes appropriate root hash
    /// from provided DataAvailabilityHeader
    #[allow(dead_code)] // unused for now
    pub fn new(
        namespace: Namespace,
        row_index: u16,
        dah: &DataAvailabilityHeader,
        block_height: u64,
    ) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        let dah_root = dah
            .root(AxisType::Row, row_index as usize)
            .ok_or(Error::EdsIndexOutOfRange(row_index as usize))?;
        let hash = Sha256::digest(dah_root.to_array()).into();

        Ok(Self {
            namespace,
            row_index,
            hash,
            block_height,
        })
    }

    /// number of bytes needed to represent `SampleId`
    pub const fn size() -> usize {
        // size of:
        // NamespacedHash<NS_SIZE> + u16 + [u8; 32] + u64
        // NS_SIZE ( = 29)         + 2   + 32       + 8
        NS_SIZE + 42
    }

    fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(NAMESPACED_DATA_ID_SIZE);

        bytes.put_u16_le(self.row_index);
        bytes.put(&self.hash[..]);
        bytes.put_u64_le(self.block_height);
        bytes.put(self.namespace.as_bytes());
    }

    fn decode(buffer: &[u8; NAMESPACED_DATA_ID_SIZE]) -> Result<Self> {
        let mut cursor = Cursor::new(buffer);

        let row_index = cursor.get_u16_le();
        let hash = cursor.copy_to_bytes(HASH_SIZE).as_ref().try_into().unwrap();

        let block_height = cursor.get_u64_le();
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        let namespace = Namespace::from_raw(cursor.copy_to_bytes(NS_SIZE).as_ref()).unwrap();

        Ok(Self {
            namespace,
            row_index,
            hash,
            block_height,
        })
    }
}

impl HasMultihash<NAMESPACED_DATA_ID_SIZE> for NamespacedDataId {
    fn multihash(&self) -> Result<Multihash<NAMESPACED_DATA_ID_SIZE>> {
        let mut bytes = BytesMut::with_capacity(NAMESPACED_DATA_ID_SIZE);
        self.encode(&mut bytes);
        // length is correct, so unwrap is safe
        Ok(Multihash::wrap(NAMESPACED_DATA_ID_MULTIHASH_CODE, &bytes[..]).unwrap())
    }
}

impl HasCid<NAMESPACED_DATA_ID_SIZE> for NamespacedDataId {
    fn codec() -> u64 {
        NAMESPACED_DATA_ID_CODEC
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for NamespacedDataId {
    type Error = Error;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != NAMESPACED_DATA_ID_CODEC {
            return Err(Error::InvalidCidCodec(codec, NAMESPACED_DATA_ID_CODEC));
        }

        let hash = cid.hash();

        let size = hash.size();
        if size as usize != NAMESPACED_DATA_ID_SIZE {
            return Err(Error::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != NAMESPACED_DATA_ID_MULTIHASH_CODE {
            return Err(Error::InvalidMultihashCode(
                code,
                NAMESPACED_DATA_ID_MULTIHASH_CODE,
            ));
        }

        NamespacedDataId::decode(hash.digest()[..NAMESPACED_DATA_ID_SIZE].try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::{NamespacedHash, NamespacedHashExt};

    #[test]
    fn round_trip() {
        let ns = Namespace::new_v0(&[0, 1]).unwrap();
        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(); 10],
            column_roots: vec![NamespacedHash::empty_root(); 10],
        };
        let data_id = NamespacedDataId::new(ns, 5, &dah, 100).unwrap();
        let cid = data_id.cid_v1().unwrap();

        let multihash = cid.hash();
        assert_eq!(multihash.code(), NAMESPACED_DATA_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), NAMESPACED_DATA_ID_SIZE as u8);

        let deserialized_data_id = NamespacedDataId::try_from(cid).unwrap();
        assert_eq!(data_id, deserialized_data_id);
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0xA0, 0xF0, 0x01, // CID codec = 7820
            0xA1, 0xF0, 0x01, // multihash code = 7821
            0x47, // len = NAMESPACED_DATA_ID_SIZE = 45
            7, 0, // row = 7
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // NS = 1
        ];

        let cid = CidGeneric::<NAMESPACED_DATA_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), NAMESPACED_DATA_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), NAMESPACED_DATA_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), NAMESPACED_DATA_ID_SIZE as u8);
        let sample_id = NamespacedDataId::try_from(cid).unwrap();
        assert_eq!(sample_id.row_index, 7);
        assert_eq!(sample_id.hash, [0xFF; 32]);
        assert_eq!(sample_id.block_height, 64);
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash =
            Multihash::<NAMESPACED_DATA_ID_SIZE>::wrap(888, &[0; NAMESPACED_DATA_ID_SIZE]).unwrap();
        let cid =
            CidGeneric::<NAMESPACED_DATA_ID_SIZE>::new_v1(NAMESPACED_DATA_ID_CODEC, multihash);
        let axis_err = NamespacedDataId::try_from(cid).unwrap_err();
        assert!(matches!(
            axis_err,
            Error::InvalidMultihashCode(888, NAMESPACED_DATA_ID_MULTIHASH_CODE)
        ));
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash = Multihash::<NAMESPACED_DATA_ID_SIZE>::wrap(
            NAMESPACED_DATA_ID_MULTIHASH_CODE,
            &[0; NAMESPACED_DATA_ID_SIZE],
        )
        .unwrap();
        let cid = CidGeneric::<NAMESPACED_DATA_ID_SIZE>::new_v1(4321, multihash);
        let axis_err = NamespacedDataId::try_from(cid).unwrap_err();
        assert!(matches!(
            axis_err,
            Error::InvalidCidCodec(4321, NAMESPACED_DATA_ID_CODEC)
        ));
    }
}
