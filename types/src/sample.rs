use std::mem::size_of;

use tendermint::Hash;
use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use celestia_proto::proof::pb::Proof as RawProof;
use celestia_proto::share::p2p::shwap::Sample as RawSample;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::nmt_proof::NamespaceProof as NmtNamespaceProof;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::row::RowId;
use crate::rsmt2d::AxisType;
use crate::nmt::{NamespaceProof, NamespacedHash, NamespacedHashExt, NamespacedSha2Hasher, Nmt};
use crate::ExtendedDataSquare;
use crate::{Error, Result, Share};

const SAMPLE_ID_SIZE: usize = SampleId::size();
pub const SAMPLE_ID_MULTIHASH_CODE: u64 = 0x7801;
pub const SAMPLE_ID_CODEC: u64 = 0x7800;

/// Represents particular sample along the axis on specific Data Square
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SampleId {
    pub row: RowId,
    pub index: u16,
}

/// Represents Sample, with proof of its inclusion and location on EDS
#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "RawSample", into = "RawSample")]
pub struct Sample {
    pub sample_id: SampleId,

    pub sample_proof_type: AxisType,
    pub share: Share,
    pub proof: NamespaceProof,
}

impl Sample {
    /// Create new sample from EDS along provided axis
    pub fn new(
        axis_type: AxisType,
        index: usize,
        eds: &ExtendedDataSquare,
        block_height: u64,
    ) -> Result<Self> {
        let square_len = eds.square_len();

        let (axis_index, sample_index) = match axis_type {
            AxisType::Row => (index / square_len, index % square_len),
            AxisType::Col => (index % square_len, index / square_len),
        };

        let shares = eds.axis(axis_type, axis_index, square_len);

        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));

        // TODO: are erasure coded shares correctly prefixed with parity namespace?
        for s in &shares {
            tree.push_leaf(s.data(), *s.namespace())
                .map_err(Error::Nmt)?;
        }

        let proof = NmtNamespaceProof::PresenceProof {
            proof: tree.build_range_proof(sample_index..sample_index + 1),
            ignore_max_ns: true,
        };

        let sample_id = SampleId::new(index, square_len, block_height)?;

        Ok(Sample {
            sample_id,
            sample_proof_type: axis_type,
            share: shares[sample_index].clone(), // or add copy to Share?
            proof: proof.into(),
        })
    }

    /// Validate sample with root hash from ExtendedHeader
    pub fn validate(&self, hash: Hash) -> Result<()> {
        let namespaced_hash = NamespacedHash::from_raw(hash.as_ref())?;
        self.proof
            .verify_range(&namespaced_hash, &[&self.share], *self.share.namespace())
            .map_err(Error::RangeProofError)
    }
}

impl Protobuf<RawSample> for Sample {}

impl TryFrom<RawSample> for Sample {
    type Error = Error;

    fn try_from(sample: RawSample) -> Result<Sample, Self::Error> {
        let Some(proof) = sample.sample_proof else {
            return Err(Error::MissingProof);
        };

        let sample_id = SampleId::decode(&sample.sample_id)?;
        let share = Share::from_raw(&sample.sample_share)?;
        let sample_proof_type = u8::try_from(sample.sample_type)
            .map_err(|_| Error::InvalidAxis(sample.sample_type))?
            .try_into()?;

        Ok(Sample {
            sample_id,
            sample_proof_type,
            share,
            proof: proof.try_into()?,
        })
    }
}

impl From<Sample> for RawSample {
    fn from(sample: Sample) -> RawSample {
        let mut sample_id_bytes = BytesMut::new();
        sample.sample_id.encode(&mut sample_id_bytes);
        let sample_proof = RawProof {
            start: sample.proof.start_idx() as i64,
            end: sample.proof.end_idx() as i64,
            nodes: sample.proof.siblings().iter().map(|h| h.to_vec()).collect(),
            leaf_hash: vec![], // this is an inclusion proof
            is_max_namespace_ignored: true,
        };

        RawSample {
            sample_id: sample_id_bytes.to_vec(),
            sample_share: sample.share.to_vec(),
            sample_type:  sample.sample_proof_type as u8 as i32, // u8::from(sample.sample_proof_type) as i32,
            sample_proof: Some(sample_proof),
        }
    }
}

impl SampleId {
    /// Create new SampleId. Index references sample number from the entire Data Square (is
    /// converted to row/col coordinates internally). Same location can be sampled row or
    /// column-wise, axis_type is used to distinguish that. Axis root hash is calculated from the
    /// DataAvailabilityHeader
    pub fn new(
        index: usize,
        square_len: usize,
        block_height: u64,
    ) -> Result<Self> {
        let row_index = index / square_len;
        let sample_index = index % square_len;

        if row_index >= square_len || sample_index >= square_len {
            return Err(Error::EdsIndexOutOfRange(index));
        }

        Ok(SampleId {
            row: RowId::new(row_index, block_height)?,
            index: sample_index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(sample_index))?,
        })
    }

    /// number of bytes needed to represent `SampleId`
    pub const fn size() -> usize {
        RowId::size() + size_of::<u16>()
    }

    fn encode(&self, bytes: &mut BytesMut) {
        self.row.encode(bytes);
        bytes.put_u16_le(self.index);
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != SAMPLE_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (row_id, index) = buffer.split_at(RowId::size());
        // RawSampleId len is defined as RowId::size + u16::size, these are safe
        Ok(Self {
            row: RowId::decode(row_id)?,
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
    use crate::nmt::Namespace;

    #[test]
    fn round_trip() {
        let sample_id = SampleId::new(5, 10, 100).unwrap();
        let cid = CidGeneric::try_from(sample_id).unwrap();

        let multihash = cid.hash();
        assert_eq!(multihash.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), SAMPLE_ID_SIZE as u8);

        let deserialized_sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id, deserialized_sample_id);
    }

    #[test]
    fn index_calculation() {
        let square_len = 8;

        SampleId::new(10, square_len, 100).unwrap();
        SampleId::new(63, square_len, 100).unwrap();
        let sample_err = SampleId::new(64, square_len, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(64)));
        let sample_err = SampleId::new(99, square_len, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(99)));
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x80, 0xF0, 0x01, // CID codec = 7800
            0x81, 0xF0, 0x01, // multihash code = 7801
            0x0C, // len = SAMPLE_ID_SIZE = 45
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            7, 0, // axis index = 7
            5, 0, // sample index = 5
        ];

        let cid = CidGeneric::<SAMPLE_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), SAMPLE_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), SAMPLE_ID_SIZE as u8);
        let sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id.row.index, 7);
        assert_eq!(sample_id.row.block_height, 64);
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

    #[test]
    fn decode_sample_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/sample.data");
        let msg = Sample::decode(&bytes[..]).unwrap();

        assert_eq!(msg.sample_id.index, 1);
        assert_eq!(msg.sample_id.row.index, 0);
        assert_eq!(msg.sample_id.row.block_height, 1);

        let ns = Namespace::new_v0(&[11, 13, 177, 159, 193, 156, 129, 121, 234, 136]).unwrap();
        assert_eq!(msg.share.namespace(), ns);
    }
}
