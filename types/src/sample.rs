use std::mem::size_of;

use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use cid::CidGeneric;
use multihash::Multihash;

use crate::axis::{AxisId, AxisType};
use crate::DataAvailabilityHeader;
use crate::{Error, Result};

const SAMPLE_ID_SIZE: usize = SampleId::size();
pub const SAMPLE_ID_MULTIHASH_CODE: u64 = 0x7801;
pub const SAMPLE_ID_CODEC: u64 = 0x7800;

/// Represents particular sample along the axis on specific Data Square
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SampleId {
    pub axis: AxisId,
    pub index: u16,
}

impl SampleId {
    /// Create new SampleId. Index references sample number from the entire Data Square (is
    /// converted to row/col coordinates internally). Same location can be sampled row or
    /// column-wise, axis_type is used to distinguish that. Axis root hash is calculated from the
    /// DataAvailabilityHeader
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

    /// number of bytes needed to represent `SampleId`
    pub const fn size() -> usize {
        AxisId::size() + size_of::<u16>()
    }

    fn encode(&self, bytes: &mut BytesMut) {
        self.axis.encode(bytes);
        bytes.put_u16_le(self.index);
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != SAMPLE_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (axis_id, index) = buffer.split_at(AxisId::size());
        // RawSampleId len is defined as AxisId::size + u16::size, these are safe
        Ok(Self {
            axis: AxisId::decode(axis_id)?,
            index: u16::from_le_bytes(index.try_into().unwrap()),
        })
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for SampleId {
    type Error = CidError;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != SAMPLE_ID_CODEC {
            return Err(CidError::InvalidCidCodec(codec));
        }

        let hash = cid.hash();

        let size = hash.size() as usize;
        if size != SAMPLE_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != SAMPLE_ID_MULTIHASH_CODE {
            return Err(CidError::InvalidMultihashCode(
                code,
                SAMPLE_ID_MULTIHASH_CODE,
            ));
        }

        SampleId::decode(hash.digest())
    }
}

impl TryFrom<SampleId> for CidGeneric<SAMPLE_ID_SIZE> {
    type Error = CidError;

    fn try_from(sample_id: SampleId) -> Result<Self, Self::Error> {
        let mut bytes = BytesMut::with_capacity(SAMPLE_ID_SIZE);
        // length is correct, so unwrap is safe
        sample_id.encode(&mut bytes);

        let mh = Multihash::wrap(SAMPLE_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        Ok(CidGeneric::new_v1(SAMPLE_ID_CODEC, mh))
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
        let cid = CidGeneric::try_from(sample_id).unwrap();

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
        assert_eq!(
            axis_err,
            CidError::InvalidMultihashCode(888, SAMPLE_ID_MULTIHASH_CODE)
        );
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash =
            Multihash::<SAMPLE_ID_SIZE>::wrap(SAMPLE_ID_MULTIHASH_CODE, &[0; SAMPLE_ID_SIZE])
                .unwrap();
        let cid = CidGeneric::<SAMPLE_ID_SIZE>::new_v1(4321, multihash);
        let axis_err = SampleId::try_from(cid).unwrap_err();
        assert!(matches!(axis_err, CidError::InvalidCidCodec(4321)));
    }
}
