use std::io::Cursor;
use std::result::Result as StdResult;

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Axis as RawAxis;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tendermint_proto::Protobuf;

use crate::nmt::{NamespacedHashExt, NamespacedSha2Hasher, Nmt, HASH_SIZE};
use crate::{DataAvailabilityHeader, ExtendedDataSquare};
use crate::{Error, Result, Share};

const AXIS_ID_SIZE: usize = AxisId::size();
pub const AXIS_ID_MULTIHASH_CODE: u64 = 0x7811;
pub const AXIS_ID_CODEC: u64 = 0x7810;

/// Represents either Column or Row of the Data Square.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AxisType {
    Row = 0,
    Col,
}

impl TryFrom<u8> for AxisType {
    type Error = Error;

    fn try_from(value: u8) -> StdResult<Self, Self::Error> {
        match value {
            0 => Ok(AxisType::Row),
            1 => Ok(AxisType::Col),
            n => Err(Error::InvalidAxis(n.into())),
        }
    }
}

/// Represents particular particular Column or Row in a specific Data Square,
/// paired together with a hash of the axis root.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct AxisId {
    pub axis_type: AxisType,
    pub index: u16,
    pub hash: [u8; HASH_SIZE],
    pub block_height: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "RawAxis", into = "RawAxis")]
pub struct Axis {
    pub axis_id: AxisId,

    pub shares: Vec<Share>,
}

impl Axis {
    pub fn new(
        axis_type: AxisType,
        index: usize,
        dah: &DataAvailabilityHeader,
        eds: &ExtendedDataSquare,
        block_height: u64,
    ) -> Result<Self> {
        let square_len = dah.square_len();

        let axis_id = AxisId::new(axis_type, index, dah, block_height)?;
        let mut shares = eds.axis(axis_type, index, square_len);
        shares.truncate(square_len / 2);

        Ok(Axis { axis_id, shares })
    }

    pub fn validate(&self) -> Result<()> {
        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));

        for s in &self.shares {
            tree.push_leaf(s.data(), *s.namespace())
                .map_err(Error::Nmt)?;
        }

        //TODO: only original data shares are sent over the wire, we need leopard codec to
        //re-compute parity shares
        /*
        let parity_shares : Vec<Share> = unimplemented!();
        for s in parity_shares {
            tree.push_leaf(s.data(), *s.namespace())
                .map_err(Error::Nmt)?;
        }
        */

        if self.axis_id.hash != tree.root().hash() {
            return Err(Error::RootMismatch);
        }

        unimplemented!("unable to compute parity shares")
    }
}

impl Protobuf<RawAxis> for Axis {}

impl TryFrom<RawAxis> for Axis {
    type Error = Error;

    fn try_from(axis: RawAxis) -> Result<Axis, Self::Error> {
        let axis_id = AxisId::decode(&axis.axis_id)?;
        let shares = axis
            .axis_half
            .into_iter()
            .map(|s| Share::from_raw(&s))
            .collect::<Result<Vec<_>>>()?;

        Ok(Axis { axis_id, shares })
    }
}

impl From<Axis> for RawAxis {
    fn from(axis: Axis) -> RawAxis {
        let mut axis_id_bytes = BytesMut::new();
        axis.axis_id.encode(&mut axis_id_bytes);

        RawAxis {
            axis_id: axis_id_bytes.to_vec(),
            axis_half: axis.shares.into_iter().map(|s| s.data.to_vec()).collect(),
        }
    }
}

impl AxisId {
    /// Create new axis for the particular data square
    pub fn new(
        axis_type: AxisType,
        index: usize,
        dah: &DataAvailabilityHeader,
        block_height: u64,
    ) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        let dah_root = dah
            .root(axis_type, index)
            .ok_or(Error::EdsIndexOutOfRange(index))?;
        let hash = Sha256::digest(dah_root.to_array()).into();

        Ok(Self {
            axis_type,
            index: index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(index))?,
            hash,
            block_height,
        })
    }

    /// Number of bytes needed to represent `AxisId`
    pub const fn size() -> usize {
        // size of:
        // u8 + u16 + [u8; 32] + u64
        //  1 +  2  +    32    +  8
        43
    }

    pub(crate) fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(AXIS_ID_SIZE);

        bytes.put_u8(self.axis_type as u8);
        bytes.put_u16_le(self.index);
        bytes.put(&self.hash[..]);
        bytes.put_u64_le(self.block_height);
    }

    pub(crate) fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != AXIS_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let mut cursor = Cursor::new(buffer);

        let axis_type =
            AxisType::try_from(cursor.get_u8()).map_err(|e| CidError::InvalidCid(e.to_string()))?;
        let index = cursor.get_u16_le();
        let hash = cursor.copy_to_bytes(HASH_SIZE).as_ref().try_into().unwrap();
        let block_height = cursor.get_u64_le();

        if block_height == 0 {
            return Err(CidError::InvalidCid("Zero block height".to_string()));
        }

        Ok(Self {
            axis_type,
            index,
            hash,
            block_height,
        })
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for AxisId {
    type Error = CidError;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != AXIS_ID_CODEC {
            return Err(CidError::InvalidCidCodec(codec));
        }

        let hash = cid.hash();

        let size = hash.size() as usize;
        if size != AXIS_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != AXIS_ID_MULTIHASH_CODE {
            return Err(CidError::InvalidMultihashCode(code, AXIS_ID_MULTIHASH_CODE));
        }

        AxisId::decode(hash.digest())
    }
}

impl TryFrom<AxisId> for CidGeneric<AXIS_ID_SIZE> {
    type Error = CidError;

    fn try_from(axis: AxisId) -> Result<Self, Self::Error> {
        let mut bytes = BytesMut::with_capacity(AXIS_ID_SIZE);
        axis.encode(&mut bytes);
        // length is correct, so unwrap is safe
        let mh = Multihash::wrap(AXIS_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        Ok(CidGeneric::new_v1(AXIS_ID_CODEC, mh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::appconsts::SHARE_SIZE;
    use crate::nmt::{Namespace, NamespacedHash, NS_SIZE};

    #[test]
    fn axis_type_serialization() {
        assert_eq!(AxisType::Row as u8, 0);
        assert_eq!(AxisType::Col as u8, 1);
    }

    #[test]
    fn axis_type_deserialization() {
        assert_eq!(AxisType::try_from(0).unwrap(), AxisType::Row);
        assert_eq!(AxisType::try_from(1).unwrap(), AxisType::Col);

        let axis_type_err = AxisType::try_from(2).unwrap_err();
        assert!(matches!(axis_type_err, Error::InvalidAxis(2)));
        let axis_type_err = AxisType::try_from(99).unwrap_err();
        assert!(matches!(axis_type_err, Error::InvalidAxis(99)));
    }

    #[test]
    fn round_trip_test() {
        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(); 10],
            column_roots: vec![NamespacedHash::empty_root(); 10],
        };
        let axis_id = AxisId::new(AxisType::Row, 5, &dah, 100).unwrap();
        let cid = CidGeneric::try_from(axis_id).unwrap();

        let multihash = cid.hash();
        assert_eq!(multihash.code(), AXIS_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), AXIS_ID_SIZE as u8);

        let deserialized_axis_id = AxisId::try_from(cid).unwrap();
        assert_eq!(axis_id, deserialized_axis_id);
    }

    #[test]
    fn index_calculation() {
        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(); 8],
            column_roots: vec![NamespacedHash::empty_root(); 8],
        };

        AxisId::new(AxisType::Row, 1, &dah, 100).unwrap();
        AxisId::new(AxisType::Row, 7, &dah, 100).unwrap();
        let axis_err = AxisId::new(AxisType::Row, 8, &dah, 100).unwrap_err();
        assert!(matches!(axis_err, Error::EdsIndexOutOfRange(8)));
        let axis_err = AxisId::new(AxisType::Row, 100, &dah, 100).unwrap_err();
        assert!(matches!(axis_err, Error::EdsIndexOutOfRange(100)));
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // multihash code = 7811
            0x2B, // len = AXIS_ID_SIZE = 43
            0,    // axis type = Row = 0
            7, 0, // axis index = 7
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
        ];

        let cid = CidGeneric::<AXIS_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), AXIS_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), AXIS_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), AXIS_ID_SIZE as u8);
        let axis_id = AxisId::try_from(cid).unwrap();
        assert_eq!(axis_id.axis_type, AxisType::Row);
        assert_eq!(axis_id.index, 7);
        assert_eq!(axis_id.hash, [0xFF; 32]);
        assert_eq!(axis_id.block_height, 64);
    }

    #[test]
    fn invalid_axis() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // code = 7811
            0x2B, // len = AXIS_ID_SIZE = 43
            0xBE, // invalid axis type!
            7, 0, // axis index = 7
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
        ];

        let cid = CidGeneric::<AXIS_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), AXIS_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), AXIS_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), AXIS_ID_SIZE as u8);
        let axis_err = AxisId::try_from(cid).unwrap_err();
        assert_eq!(
            axis_err,
            CidError::InvalidCid("Invalid axis type: 190".to_string())
        );
    }

    #[test]
    fn zero_block_height() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // code = 7811
            0x2B, // len = AXIS_ID_SIZE = 43
            0,    // axis type = Row = 0
            7, 0, // axis index = 7
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
            0, 0, 0, 0, 0, 0, 0, 0, // invalid block height = 0 !
        ];

        let cid = CidGeneric::<AXIS_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), AXIS_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), AXIS_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), AXIS_ID_SIZE as u8);
        let axis_err = AxisId::try_from(cid).unwrap_err();
        assert_eq!(
            axis_err,
            CidError::InvalidCid("Zero block height".to_string())
        );
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash = Multihash::<AXIS_ID_SIZE>::wrap(999, &[0; AXIS_ID_SIZE]).unwrap();
        let cid = CidGeneric::<AXIS_ID_SIZE>::new_v1(AXIS_ID_CODEC, multihash);
        let axis_err = AxisId::try_from(cid).unwrap_err();
        assert_eq!(
            axis_err,
            CidError::InvalidMultihashCode(999, AXIS_ID_MULTIHASH_CODE)
        );
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash =
            Multihash::<AXIS_ID_SIZE>::wrap(AXIS_ID_MULTIHASH_CODE, &[0; AXIS_ID_SIZE]).unwrap();
        let cid = CidGeneric::<AXIS_ID_SIZE>::new_v1(1234, multihash);
        let axis_err = AxisId::try_from(cid).unwrap_err();
        assert_eq!(axis_err, CidError::InvalidCidCodec(1234));
    }

    #[test]
    fn decode_axis_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/axis.data");
        let axis = Axis::decode(&bytes[..]).unwrap();

        /*
        msg.axis_id.axis_type = AxisType::Col;
        msg.axis_id.index = 64;
        msg.axis_id.hash = [0xEF; HASH_SIZE];
        msg.axis_id.block_height = 255;


        let mut i = 0;
        for share in &mut msg.shares {
            let ns = Namespace::new_v0(&[i]).unwrap();

            let data = [0; crate::consts::appconsts::SHARE_SIZE];
            share.data[..].copy_from_slice(&data);
            share.data[..NS_SIZE].copy_from_slice(ns.as_bytes());

            println!("{i} {:?}", share.namespace());
            i += 1;
        }
        let mut file = std::fs::File::create("axis.data2").unwrap();
        let bytes = msg.encode_vec().unwrap();
        file.write_all(&bytes).unwrap();
        */
        assert_eq!(axis.axis_id.axis_type, AxisType::Col);
        assert_eq!(axis.axis_id.index, 64);
        assert_eq!(axis.axis_id.hash, [0xEF; HASH_SIZE]);
        assert_eq!(axis.axis_id.block_height, 255);

        for (idx, share) in axis.shares.iter().enumerate() {
            let ns = Namespace::new_v0(&[idx as u8]).unwrap();
            assert_eq!(share.namespace(), ns);
            let data = [0; SHARE_SIZE - NS_SIZE];
            assert_eq!(share.data(), data);
        }
    }
}
