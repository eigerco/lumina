use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Data as RawNamespacedData;
use cid::CidGeneric;
use multihash::Multihash;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::nmt::{Namespace, NamespaceProof, NS_SIZE};
use crate::row::RowId;
use crate::{Error, Result};

const NAMESPACED_DATA_ID_SIZE: usize = NamespacedDataId::size();
pub const NAMESPACED_DATA_ID_MULTIHASH_CODE: u64 = 0x7821;
pub const NAMESPACED_DATA_ID_CODEC: u64 = 0x7820;

/// Represents shares from a namespace located on a particular row of Data Square
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct NamespacedDataId {
    /// Row on which the data is located on
    pub row: RowId,
    /// Namespace data belongs to
    pub namespace: Namespace,
}

/// `NamespacedData` is constructed out of the ExtendedDataSquare, containing up to a row of
/// shares belonging to a particular namespace and a proof of their inclusion. If, for
/// particular EDS, shares from the namespace span multiple rows, one needs multiple
/// NamespacedData instances to cover the whole range.
#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "RawNamespacedData", into = "RawNamespacedData")]
pub struct NamespacedData {
    /// Location of the shares on EDS
    pub namespaced_data_id: NamespacedDataId,
    /// Proof of data inclusion
    pub proof: NamespaceProof,
    /// Shares with data
    pub shares: Vec<Vec<u8>>,
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

        Ok(NamespacedData {
            namespaced_data_id,
            shares: namespaced_data.data_shares,
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
    pub fn new(namespace: Namespace, row_index: usize, block_height: u64) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        Ok(Self {
            row: RowId::new(row_index, block_height)?,
            namespace,
        })
    }

    /// number of bytes needed to represent `SampleId`
    pub const fn size() -> usize {
        // size of:
        // RowId + Namespace
        //    10 +        29 = 39
        RowId::size() + NS_SIZE
    }

    fn encode(&self, bytes: &mut BytesMut) {
        self.row.encode(bytes);
        bytes.put(self.namespace.as_bytes());
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != NAMESPACED_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (row_id, namespace) = buffer.split_at(RowId::size());

        Ok(Self {
            row: RowId::decode(row_id)?,
            namespace: Namespace::from_raw(namespace)
                .map_err(|e| CidError::InvalidCid(e.to_string()))?,
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
        // length is correct, so the unwrap is safe
        let mh = Multihash::wrap(NAMESPACED_DATA_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        Ok(CidGeneric::new_v1(NAMESPACED_DATA_ID_CODEC, mh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Share;

    #[test]
    fn round_trip() {
        let ns = Namespace::new_v0(&[0, 1]).unwrap();
        let data_id = NamespacedDataId::new(ns, 5, 100).unwrap();
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
            0x27, // len = NAMESPACED_DATA_ID_SIZE = 39
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            7, 0, // row = 7
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // NS = 1
        ];

        let cid = CidGeneric::<NAMESPACED_DATA_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), NAMESPACED_DATA_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), NAMESPACED_DATA_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), NAMESPACED_DATA_ID_SIZE as u8);
        let data_id = NamespacedDataId::try_from(cid).unwrap();
        assert_eq!(data_id.namespace, Namespace::new_v0(&[1]).unwrap());
        assert_eq!(data_id.row.block_height, 64);
        assert_eq!(data_id.row.index, 7);
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

        let ns = Namespace::new_v0(&[135, 30, 47, 81, 60, 66, 177, 20, 57, 85]).unwrap();
        assert_eq!(msg.namespaced_data_id.namespace, ns);
        assert_eq!(msg.namespaced_data_id.row.index, 0);
        assert_eq!(msg.namespaced_data_id.row.block_height, 1);

        for s in msg.shares {
            let s = Share::from_raw(&s).unwrap();
            assert_eq!(s.namespace(), ns);
        }
    }
}
