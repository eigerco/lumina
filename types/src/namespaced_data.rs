use std::io::Cursor;

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Data as RawNamespacedData;
use cid::CidGeneric;
use multihash::Multihash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tendermint_proto::Protobuf;

use crate::extended_data_square::AxisType;
use crate::nmt::{Namespace, NamespaceProof, NamespacedHashExt, HASH_SIZE, NS_SIZE};
use crate::{DataAvailabilityHeader, Error, Result, Share};

const NAMESPACED_DATA_ID_SIZE: usize = NamespacedDataId::size();
pub const NAMESPACED_DATA_ID_MULTIHASH_CODE: u64 = 0x7821;
pub const NAMESPACED_DATA_ID_CODEC: u64 = 0x7820;

/// Represents shares from a namespace located on a particular row of Data Square
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct NamespacedDataId {
    pub namespace: Namespace,
    pub row_index: u16,
    pub hash: [u8; HASH_SIZE],
    pub block_height: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "RawNamespacedData", into = "RawNamespacedData")]
pub struct NamespacedData {
    pub namespaced_data_id: NamespacedDataId,

    pub proof: NamespaceProof,
    pub shares: Vec<Share>,
}

impl NamespacedData {}

impl Protobuf<RawNamespacedData> for NamespacedData {}

impl TryFrom<RawNamespacedData> for NamespacedData {
    type Error = Error;

    fn try_from(namespaced_data: RawNamespacedData) -> Result<NamespacedData, Self::Error> {
        let Some(proof) = namespaced_data.data_proof else {
            return Err(Error::MissingProof);
        };

        let namespaced_data_id = NamespacedDataId::decode(&namespaced_data.data_id)?;
        let shares = namespaced_data
            .data_shares
            .iter()
            .map(|s| Share::from_raw(s))
            .collect::<Result<_, _>>()?;

        Ok(NamespacedData {
            namespaced_data_id,
            shares,
            proof: proof.try_into()?,
        })
    }
}

impl From<NamespacedData> for RawNamespacedData {
    fn from(namespaced_data: NamespacedData) -> RawNamespacedData {
        let mut data_id_bytes = BytesMut::new();
        namespaced_data
            .namespaced_data_id
            .encode(&mut data_id_bytes);

        RawNamespacedData {
            data_id: data_id_bytes.to_vec(),
            data_shares: namespaced_data.shares.iter().map(|s| s.to_vec()).collect(),
            data_proof: Some(namespaced_data.proof.into()),
        }
    }
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

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != NAMESPACED_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let mut cursor = Cursor::new(buffer);

        let row_index = cursor.get_u16_le();
        let hash = cursor.copy_to_bytes(HASH_SIZE).as_ref().try_into().unwrap();

        let block_height = cursor.get_u64_le();
        if block_height == 0 {
            return Err(CidError::InvalidCid("Zero block height".to_string()));
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

impl<const S: usize> TryFrom<CidGeneric<S>> for NamespacedDataId {
    type Error = CidError;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != NAMESPACED_DATA_ID_CODEC {
            return Err(CidError::InvalidCidCodec(codec));
        }

        let hash = cid.hash();

        let size = hash.size() as usize;
        if size != NAMESPACED_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != NAMESPACED_DATA_ID_MULTIHASH_CODE {
            return Err(CidError::InvalidMultihashCode(
                code,
                NAMESPACED_DATA_ID_MULTIHASH_CODE,
            ));
        }

        NamespacedDataId::decode(hash.digest())
    }
}

impl TryFrom<NamespacedDataId> for CidGeneric<NAMESPACED_DATA_ID_SIZE> {
    type Error = CidError;

    fn try_from(namespaced_data_id: NamespacedDataId) -> Result<Self, Self::Error> {
        let mut bytes = BytesMut::with_capacity(NAMESPACED_DATA_ID_SIZE);
        namespaced_data_id.encode(&mut bytes);
        // length is correct, so unwrap is safe
        let mh = Multihash::wrap(NAMESPACED_DATA_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        Ok(CidGeneric::new_v1(NAMESPACED_DATA_ID_CODEC, mh))
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
        let cid = CidGeneric::try_from(data_id).unwrap();

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
        let data_id = NamespacedDataId::try_from(cid).unwrap();
        assert_eq!(data_id.row_index, 7);
        assert_eq!(data_id.hash, [0xFF; 32]);
        assert_eq!(data_id.block_height, 64);
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash =
            Multihash::<NAMESPACED_DATA_ID_SIZE>::wrap(888, &[0; NAMESPACED_DATA_ID_SIZE]).unwrap();
        let cid =
            CidGeneric::<NAMESPACED_DATA_ID_SIZE>::new_v1(NAMESPACED_DATA_ID_CODEC, multihash);
        let axis_err = NamespacedDataId::try_from(cid).unwrap_err();
        assert_eq!(
            axis_err,
            CidError::InvalidMultihashCode(888, NAMESPACED_DATA_ID_MULTIHASH_CODE)
        );
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
        assert_eq!(axis_err, CidError::InvalidCidCodec(4321));
    }

    #[test]
    fn decode_data_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/namespaced_data.data");
        let msg = NamespacedData::decode(&bytes[..]).unwrap();

        let ns = Namespace::new_v0(&[93, 2, 248, 47, 108, 59, 195, 216, 222, 33]).unwrap();
        assert_eq!(msg.namespaced_data_id.namespace, ns);
        assert_eq!(msg.namespaced_data_id.row_index, 0);
        assert_eq!(msg.namespaced_data_id.block_height, 1);

        for s in msg.shares {
            assert_eq!(s.namespace(), ns);
        }
    }
}
