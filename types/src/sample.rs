use std::mem::size_of;

use bytes::{BufMut, BytesMut};
use cid::CidGeneric;
use multihash::Multihash;

use crate::axis::{AxisId, AxisType};
use crate::multihash::{HasCid, HasMultihash};
use crate::DataAvailabilityHeader;
use crate::{Error, Result};

const SAMPLE_ID_SIZE: usize = SampleId::size();
pub const SAMPLE_ID_MULTIHASH_CODE: u64 = 0x7801;
pub const SAMPLE_ID_CODEC: u64 = 0x7800;

pub type RawSampleId = [u8; SAMPLE_ID_SIZE];

#[derive(Debug, PartialEq)]
pub struct SampleId {
    pub axis: AxisId,
    pub index: u16,
}

impl SampleId {
    pub const fn size() -> usize {
        AxisId::size() + size_of::<u16>()
    }

    #[allow(dead_code)] // unused for now
    pub fn new(
        axis_type: AxisType,
        index: usize,
        dah: &DataAvailabilityHeader,
        block_height: u64,
    ) -> Result<Self> {
        let square_len = dah.square_len();

        let (axis_index, sample_index) = match axis_type {
            AxisType::Row => (index / square_len, index % square_len),
            AxisType::Col => (index % square_len, index / square_len),
        };

        Ok(SampleId {
            axis: AxisId::new(axis_type, axis_index, dah, block_height)?,
            index: sample_index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(sample_index))?,
        })
    }

    fn to_bytes(&self) -> RawSampleId {
        let mut bytes = BytesMut::with_capacity(Self::size());

        // TODO: avoid alloc?
        let axis_id_bytes = self.axis.to_bytes();
        bytes.put(&axis_id_bytes[..]);
        bytes.put_u16_le(self.index);
        bytes.as_ref().try_into().unwrap()
    }

    fn from_bytes(buffer: &RawSampleId) -> Result<Self> {
        let (axis_id, index) = buffer.split_at(AxisId::size());
        // RawSampleId len is defined as AxisId::size + u16::size, these are safe
        Ok(Self {
            axis: AxisId::from_bytes(axis_id.try_into().unwrap())?,
            index: u16::from_le_bytes(index.try_into().unwrap()),
        })
    }
}

impl HasMultihash<SAMPLE_ID_SIZE> for SampleId {
    fn multihash(&self) -> Result<Multihash<SAMPLE_ID_SIZE>> {
        let digest_bytes = self.to_bytes();
        Ok(Multihash::<SAMPLE_ID_SIZE>::wrap(
            SAMPLE_ID_MULTIHASH_CODE,
            &digest_bytes,
        )?)
    }
}

impl HasCid<SAMPLE_ID_SIZE> for SampleId {
    fn codec() -> u64 {
        SAMPLE_ID_CODEC
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for SampleId {
    type Error = Error;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != SAMPLE_ID_CODEC {
            return Err(Error::InvalidCidCodec(codec, SAMPLE_ID_CODEC));
        }

        let hash = cid.hash();

        let size = hash.size();
        if size as usize != SAMPLE_ID_SIZE {
            return Err(Error::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != SAMPLE_ID_MULTIHASH_CODE {
            return Err(Error::InvalidMultihashCode(code, SAMPLE_ID_MULTIHASH_CODE));
        }

        SampleId::from_bytes(hash.digest()[..SAMPLE_ID_SIZE].try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::{NamespacedHash, NamespacedHashExt};

    #[test]
    fn round_trip() {
        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(); 10],
            column_roots: vec![NamespacedHash::empty_root(); 10],
        };
        let sample_id = SampleId::new(AxisType::Row, 5, &dah, 100).unwrap();
        let cid = sample_id.cid_v1().unwrap();

        let multihash = cid.hash();
        assert_eq!(multihash.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), SAMPLE_ID_SIZE as u8);

        let deserialized_sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id, deserialized_sample_id);
    }

    #[test]
    fn index_calculation() {
        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(); 8],
            column_roots: vec![NamespacedHash::empty_root(); 8],
        };

        SampleId::new(AxisType::Row, 10, &dah, 100).unwrap();
        SampleId::new(AxisType::Row, 63, &dah, 100).unwrap();
        let sample_err = SampleId::new(AxisType::Row, 64, &dah, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(8)));
        let sample_err = SampleId::new(AxisType::Row, 99, &dah, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(12)));
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x80, 0xF0, 0x01, // CID codec = 7800
            0x81, 0xF0, 0x01, // multihash code = 7801
            0x2D, // len = SAMPLE_ID_SIZE = 45
            0,    // axis type = Row = 0
            7, 0, // axis index = 7
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            5, 0, // sample index = 5
        ];

        let cid = CidGeneric::<SAMPLE_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), SAMPLE_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), SAMPLE_ID_SIZE as u8);
        let sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id.axis.axis_type, AxisType::Row);
        assert_eq!(sample_id.axis.index, 7);
        assert_eq!(sample_id.axis.hash, [0xFF; 32]);
        assert_eq!(sample_id.axis.block_height, 64);
        assert_eq!(sample_id.index, 5);
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash = Multihash::<SAMPLE_ID_SIZE>::wrap(888, &[0; SAMPLE_ID_SIZE]).unwrap();
        let cid = CidGeneric::<SAMPLE_ID_SIZE>::new_v1(SAMPLE_ID_CODEC, multihash);
        let axis_err = SampleId::try_from(cid).unwrap_err();
        assert!(matches!(
            axis_err,
            Error::InvalidMultihashCode(888, SAMPLE_ID_MULTIHASH_CODE)
        ));
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash =
            Multihash::<SAMPLE_ID_SIZE>::wrap(SAMPLE_ID_MULTIHASH_CODE, &[0; SAMPLE_ID_SIZE])
                .unwrap();
        let cid = CidGeneric::<SAMPLE_ID_SIZE>::new_v1(4321, multihash);
        let axis_err = SampleId::try_from(cid).unwrap_err();
        assert!(matches!(
            axis_err,
            Error::InvalidCidCodec(4321, SAMPLE_ID_CODEC)
        ));
    }
}
