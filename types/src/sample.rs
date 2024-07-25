//! Types related to samples.
//!
//! Sample in Celestia is understood as a single [`Share`] located at an
//! index in the particular [`row`] of the [`ExtendedDataSquare`].
//!
//! [`row`]: crate::row
//! [`Share`]: crate::Share
//! [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::{ProofType as RawProofType, Sample as RawSample};
use celestia_tendermint_proto::Protobuf;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::nmt_proof::NamespaceProof as NmtNamespaceProof;
use serde::{Deserialize, Serialize};

use crate::nmt::{Namespace, NamespaceProof, NS_SIZE};
use crate::row::{RowId, ROW_ID_SIZE};
use crate::rsmt2d::{is_ods_square, AxisType, ExtendedDataSquare};
use crate::{DataAvailabilityHeader, Error, Result};

/// Number of bytes needed to represent [`SampleId`] in `multihash`.
const SAMPLE_ID_SIZE: usize = 12;
/// The code of the [`SampleId`] hashing algorithm in `multihash`.
pub const SAMPLE_ID_MULTIHASH_CODE: u64 = 0x7801;
/// The id of codec used for the [`SampleId`] in `Cid`s.
pub const SAMPLE_ID_CODEC: u64 = 0x7800;

/// Identifies a particular [`Share`] located in the [`ExtendedDataSquare`].
///
/// [`Share`]: crate::Share
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SampleId {
    row_id: RowId,
    column_index: u16,
}

/// Represents Sample, with proof of its inclusion and location on EDS
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(try_from = "RawSample", into = "RawSample")]
pub struct Sample {
    /// Location of the sample in the EDS and associated block height
    pub id: SampleId,
    /// Indication whether sampling was done row or column-wise
    pub proof_type: AxisType,
    /// Share that is being sampled
    pub share: Vec<u8>,
    /// Proof of the inclusion of the share
    pub proof: NamespaceProof,
}

impl Sample {
    /// Create a new [`Sample`] for the given index of the [`ExtendedDataSquare`] in a block.
    ///
    /// `row_index` and `column_index` specifies the [`Share`] position in EDS.
    /// `proof_type` determines whether proof of inclusion of the [`Share`] should be
    /// constructed for its row or column.
    ///
    /// # Errors
    ///
    /// This function will return an error, if:
    ///
    /// - `row_index`/`column_index` falls outside the provided [`ExtendedDataSquare`].
    /// - [`ExtendedDataSquare`] is incorrect (either data shares don't have their namespace
    ///   prefixed, or [`Share`]s aren't namespace ordered)
    /// - Block height is zero
    ///
    /// # Example
    ///
    /// ```no_run
    /// use celestia_types::AxisType;
    /// use celestia_types::sample::Sample;
    /// # use celestia_types::{ExtendedDataSquare, ExtendedHeader};
    /// #
    /// # fn get_extended_data_square(height: u64) -> ExtendedDataSquare {
    /// #    unimplemented!()
    /// # }
    /// #
    /// # fn get_extended_header(height: u64) -> ExtendedHeader {
    /// #    unimplemented!()
    /// # }
    ///
    /// let block_height = 15;
    /// let eds = get_extended_data_square(block_height);
    /// let header = get_extended_header(block_height);
    ///
    /// let sample = Sample::new(2, 3, AxisType::Row, &eds, block_height).unwrap();
    ///
    /// sample.verify(&header.dah).unwrap();
    /// ```
    ///
    /// [`Share`]: crate::Share
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn new(
        row_index: u16,
        column_index: u16,
        proof_type: AxisType,
        eds: &ExtendedDataSquare,
        block_height: u64,
    ) -> Result<Self> {
        let share = eds.share(row_index, column_index)?.to_owned();
        let id = SampleId::new(row_index, column_index, block_height)?;

        let range_proof = match proof_type {
            AxisType::Row => eds
                .row_nmt(row_index)?
                .build_range_proof(usize::from(column_index)..usize::from(column_index) + 1),
            AxisType::Col => eds
                .column_nmt(column_index)?
                .build_range_proof(usize::from(row_index)..usize::from(row_index) + 1),
        };

        let proof = NmtNamespaceProof::PresenceProof {
            proof: range_proof,
            ignore_max_ns: true,
        };

        Ok(Sample {
            id,
            proof_type,
            share,
            proof: proof.into(),
        })
    }

    /// verify sample with root hash from ExtendedHeader
    pub fn verify(&self, dah: &DataAvailabilityHeader) -> Result<()> {
        let root = match self.proof_type {
            AxisType::Row => dah
                .row_root(self.id.row_index())
                .ok_or(Error::EdsIndexOutOfRange(self.id.row_index(), 0))?,
            AxisType::Col => dah
                .column_root(self.id.column_index())
                .ok_or(Error::EdsIndexOutOfRange(0, self.id.column_index()))?,
        };

        let ns = if is_ods_square(
            self.id.row_index(),
            self.id.column_index(),
            dah.square_width(),
        ) {
            Namespace::from_raw(&self.share[..NS_SIZE])?
        } else {
            Namespace::PARITY_SHARE
        };

        self.proof
            .verify_range(&root, &[&self.share], *ns)
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

        let id = SampleId::decode(&sample.sample_id)?;

        let proof_type = match RawProofType::try_from(sample.proof_type) {
            Ok(RawProofType::RowProofType) => AxisType::Row,
            Ok(RawProofType::ColProofType) => AxisType::Col,
            Err(_) => return Err(Error::InvalidShwapProofType(sample.proof_type)),
        };

        Ok(Sample {
            id,
            proof_type,
            share: sample.sample_share,
            proof: proof.try_into()?,
        })
    }
}

impl From<Sample> for RawSample {
    fn from(sample: Sample) -> RawSample {
        let mut sample_id_bytes = BytesMut::with_capacity(SAMPLE_ID_SIZE);
        sample.id.encode(&mut sample_id_bytes);

        RawSample {
            sample_id: sample_id_bytes.to_vec(),
            sample_share: sample.share.to_vec(),
            sample_proof: Some(sample.proof.into()),
            proof_type: match sample.proof_type {
                AxisType::Row => RawProofType::RowProofType.into(),
                AxisType::Col => RawProofType::ColProofType.into(),
            },
        }
    }
}

impl SampleId {
    /// Create a new [`SampleId`] for the given `row_index` and `column_index` of the
    /// [`ExtendedDataSquare`] in a block.
    ///
    /// # Errors
    ///
    /// This function will return an error if the block height is zero.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use celestia_types::sample::SampleId;
    ///
    /// // Consider an 64 share EDS with block height of 15
    /// let header_height = 15;
    ///
    /// SampleId::new(2, 1, header_height).unwrap();
    /// ```
    ///
    /// [`Share`]: crate::Share
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn new(row_index: u16, column_index: u16, block_height: u64) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        Ok(SampleId {
            row_id: RowId::new(row_index, block_height)?,
            column_index,
        })
    }

    /// A height of the block which contains the sample.
    pub fn block_height(&self) -> u64 {
        self.row_id.block_height()
    }

    /// Row index of the [`ExtendedDataSquare`] that sample is located on.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn row_index(&self) -> u16 {
        self.row_id.index()
    }

    /// Column index of the [`ExtendedDataSquare`] that sample is located on.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn column_index(&self) -> u16 {
        self.column_index
    }

    fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(SAMPLE_ID_SIZE);
        self.row_id.encode(bytes);
        bytes.put_u16(self.column_index);
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != SAMPLE_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (row_bytes, mut col_bytes) = buffer.split_at(ROW_ID_SIZE);
        let row_id = RowId::decode(row_bytes)?;
        let column_index = col_bytes.get_u16();

        Ok(SampleId {
            row_id,
            column_index,
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

impl From<SampleId> for CidGeneric<SAMPLE_ID_SIZE> {
    fn from(sample_id: SampleId) -> Self {
        let mut bytes = BytesMut::with_capacity(SAMPLE_ID_SIZE);
        // length is correct, so unwrap is safe
        sample_id.encode(&mut bytes);

        let mh = Multihash::wrap(SAMPLE_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        CidGeneric::new_v1(SAMPLE_ID_CODEC, mh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::Namespace;
    use crate::test_utils::generate_eds;

    #[test]
    fn round_trip() {
        let sample_id = SampleId::new(5, 10, 100).unwrap();
        let cid = CidGeneric::from(sample_id);

        let multihash = cid.hash();
        assert_eq!(multihash.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), SAMPLE_ID_SIZE as u8);

        let deserialized_sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id, deserialized_sample_id);
    }

    #[test]
    fn index_calculation() {
        let eds = generate_eds(8);

        Sample::new(0, 0, AxisType::Row, &eds, 100).unwrap();
        Sample::new(7, 6, AxisType::Row, &eds, 100).unwrap();
        Sample::new(7, 7, AxisType::Row, &eds, 100).unwrap();

        let sample_err = Sample::new(7, 8, AxisType::Row, &eds, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(7, 8)));

        let sample_err = Sample::new(12, 3, AxisType::Row, &eds, 100).unwrap_err();
        assert!(matches!(sample_err, Error::EdsIndexOutOfRange(12, 3)));
    }

    #[test]
    fn sample_id_size() {
        // Size MUST be 12 by the spec.
        assert_eq!(SAMPLE_ID_SIZE, 12);

        let sample_id = SampleId::new(0, 4, 1).unwrap();
        let mut bytes = BytesMut::new();
        sample_id.encode(&mut bytes);
        assert_eq!(bytes.len(), SAMPLE_ID_SIZE);
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x80, 0xF0, 0x01, // CID codec = 7800
            0x81, 0xF0, 0x01, // multihash code = 7801
            0x0C, // len = SAMPLE_ID_SIZE = 12
            0, 0, 0, 0, 0, 0, 0, 64, // block height = 64
            0, 7, // row index = 7
            0, 5, // sample index = 5
        ];

        let cid = CidGeneric::<SAMPLE_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), SAMPLE_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), SAMPLE_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), SAMPLE_ID_SIZE as u8);
        let sample_id = SampleId::try_from(cid).unwrap();
        assert_eq!(sample_id.block_height(), 64);
        assert_eq!(sample_id.row_index(), 7);
        assert_eq!(sample_id.column_index(), 5);
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash = Multihash::<SAMPLE_ID_SIZE>::wrap(888, &[0; SAMPLE_ID_SIZE]).unwrap();
        let cid = CidGeneric::<SAMPLE_ID_SIZE>::new_v1(SAMPLE_ID_CODEC, multihash);
        let code_err = SampleId::try_from(cid).unwrap_err();
        assert_eq!(
            code_err,
            CidError::InvalidMultihashCode(888, SAMPLE_ID_MULTIHASH_CODE)
        );
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash =
            Multihash::<SAMPLE_ID_SIZE>::wrap(SAMPLE_ID_MULTIHASH_CODE, &[0; SAMPLE_ID_SIZE])
                .unwrap();
        let cid = CidGeneric::<SAMPLE_ID_SIZE>::new_v1(4321, multihash);
        let codec_err = SampleId::try_from(cid).unwrap_err();
        assert!(matches!(codec_err, CidError::InvalidCidCodec(4321)));
    }

    #[test]
    fn decode_sample_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/sample.data");
        let msg = Sample::decode(&bytes[..]).unwrap();

        assert_eq!(msg.id.column_index(), 1);
        assert_eq!(msg.id.row_index(), 0);
        assert_eq!(msg.id.block_height(), 1);

        let expected_ns =
            Namespace::new_v0(&[11, 13, 177, 159, 193, 156, 129, 121, 234, 136]).unwrap();
        let ns = Namespace::from_raw(&msg.share[..NS_SIZE]).unwrap();
        assert_eq!(ns, expected_ns);
    }
}
