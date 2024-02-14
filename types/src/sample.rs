//! Types related to samples.
//!
//! Sample in Celestia is understood as a single [`Share`] located at an
//! index in the particular [`row`] of the [`ExtendedDataSquare`].
//!
//! [`row`]: crate::row
//! [`Share`]: crate::Share
//! [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare

use std::mem::size_of;

use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Sample as RawSample;
use celestia_tendermint_proto::Protobuf;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::nmt_proof::NamespaceProof as NmtNamespaceProof;
use serde::{Deserialize, Serialize};

use crate::nmt::{Namespace, NamespaceProof, NS_SIZE};
use crate::row::RowId;
use crate::rsmt2d::{is_ods_square, AxisType, ExtendedDataSquare};
use crate::{DataAvailabilityHeader, Error, Result};

/// The size of the [`SampleId`] hash in `multihash`.
const SAMPLE_ID_SIZE: usize = SampleId::size();
/// The code of the [`SampleId`] hashing algorithm in `multihash`.
pub const SAMPLE_ID_MULTIHASH_CODE: u64 = 0x7801;
/// The id of codec used for the [`SampleId`] in `Cid`s.
pub const SAMPLE_ID_CODEC: u64 = 0x7800;

/// Identifies a particular [`Share`] located in the [`row`] of the [`ExtendedDataSquare`].
///
/// [`row`]: crate::row
/// [`Share`]: crate::Share
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SampleId {
    /// Row of the EDS sample is located on
    pub row: RowId,
    /// Index in the row of the share being sampled
    pub index: u16,
}

/// Represents Sample, with proof of its inclusion and location on EDS
#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "RawSample", into = "RawSample")]
pub struct Sample {
    /// Location of the sample in the EDS and associated block height
    pub sample_id: SampleId,
    /// Indication whether sampling was done row or column-wise
    pub sample_proof_type: AxisType,
    /// Share that is being sampled
    pub share: Vec<u8>,
    /// Proof of the inclusion of the share
    pub proof: NamespaceProof,
}

impl Sample {
    /// Create a new [`Sample`] for the given index of the [`ExtendedDataSquare`] in a block.
    ///
    /// `index` specifies the [`Share`] position in EDS, for details see [`SampleId::new`].
    /// `axis_type` determines whether proof of inclusion of the [`Share`] should be
    /// constructed for its row or column.
    ///
    /// # Errors
    /// This function will return an error, if:
    /// - `index` falls outside the provided [`ExtendedDataSquare`]
    /// - [`ExtendedDataSquare`] is incorrect (either data shares don't have their namespace
    /// prefixed, or [`Share`]s aren't namespace ordered)
    /// - block height is zero
    ///
    /// # Example
    ///
    /// ```no_run
    /// use celestia_types::AxisType;
    /// use celestia_types::sample::Sample;
    /// # use celestia_types::{ExtendedDataSquare, ExtendedHeader};
    /// # fn get_extended_data_square(height: usize) -> ExtendedDataSquare {
    /// #    unimplemented!()
    /// # }
    /// # fn get_extended_header(height: usize) -> ExtendedHeader {
    /// #    unimplemented!()
    /// # }
    ///
    /// let block_height = 15;
    /// let eds = get_extended_data_square(block_height);
    /// let index = 2 * eds.square_len() + 3; // 3rd row and 4th column as these are 0 indexed
    ///
    /// let header = get_extended_header(block_height);
    ///
    /// let sample = Sample::new(AxisType::Row, index, &eds, block_height as u64).unwrap();
    ///
    /// sample.verify(&header.dah).unwrap();
    /// ```
    ///
    /// [`Share`]: crate::Share
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
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

        let (row_index, column_index) = match axis_type {
            AxisType::Row => (axis_index, sample_index),
            AxisType::Col => (sample_index, axis_index),
        };

        let share = eds.share(row_index, column_index)?.to_owned();

        let mut tree = eds.axis_nmt(axis_type, axis_index)?;

        let proof = NmtNamespaceProof::PresenceProof {
            proof: tree.build_range_proof(sample_index..sample_index + 1),
            ignore_max_ns: true,
        };

        let sample_id = SampleId::new(index, square_len, block_height)?;

        Ok(Sample {
            sample_id,
            sample_proof_type: axis_type,
            share,
            proof: proof.into(),
        })
    }

    /// verify sample with root hash from ExtendedHeader
    pub fn verify(&self, dah: &DataAvailabilityHeader) -> Result<()> {
        let index = match self.sample_proof_type {
            AxisType::Row => self.sample_id.row.index,
            AxisType::Col => self.sample_id.index,
        }
        .into();

        let root = dah
            .root(self.sample_proof_type, index)
            .ok_or(Error::EdsIndexOutOfRange(index))?;

        let ns = if is_ods_square(
            self.sample_id.row.index.into(),
            self.sample_id.index.into(),
            dah.square_len(),
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

        let sample_id = SampleId::decode(&sample.sample_id)?;
        let sample_proof_type = u8::try_from(sample.sample_type)
            .map_err(|_| Error::InvalidAxis(sample.sample_type))?
            .try_into()?;

        Ok(Sample {
            sample_id,
            sample_proof_type,
            share: sample.sample_share,
            proof: proof.try_into()?,
        })
    }
}

impl From<Sample> for RawSample {
    fn from(sample: Sample) -> RawSample {
        let mut sample_id_bytes = BytesMut::with_capacity(SAMPLE_ID_SIZE);
        sample.sample_id.encode(&mut sample_id_bytes);

        RawSample {
            sample_id: sample_id_bytes.to_vec(),
            sample_share: sample.share.to_vec(),
            sample_type: sample.sample_proof_type as u8 as i32,
            sample_proof: Some(sample.proof.into()),
        }
    }
}

impl SampleId {
    /// Create a new [`SampleId`] for the given index of the [`ExtendedDataSquare`] in a block.
    ///
    /// When creating the [`SampleId`], [`ExtendedDataSquare`] is indexed in a row-major order,
    /// meaning that to get [`Share`] at coordinates `(row_id, col_id)`, one would pass
    /// `index = row_id * square_len + col_id`
    ///
    /// # Errors
    ///
    /// This function will return an error if the block height or sample index is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use celestia_types::sample::SampleId;
    ///
    /// // Consider an 64 share EDS with block height of 15
    /// let square_width = 8;
    /// let header_height = 15;
    ///
    /// // Create an id of a sample at the 3rd row and 2nd column
    /// // those are indexed from 0
    /// let row = 2;
    /// let col = 1;
    /// let sample_id = SampleId::new(
    ///     square_width * row + col,
    ///     square_width,
    ///     header_height,
    /// ).unwrap();
    ///
    /// assert_eq!(sample_id.row.index, row as u16);
    /// assert_eq!(sample_id.index, col as u16);
    /// ```
    ///
    /// [`Share`]: crate::Share
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn new(index: usize, square_len: usize, block_height: u64) -> Result<Self> {
        let row_index = index / square_len;
        let sample_index = index % square_len;

        if row_index >= square_len || sample_index >= square_len {
            return Err(Error::EdsIndexOutOfRange(index));
        }

        let row_id = RowId::new(
            row_index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(index))?,
            block_height,
        )?;

        Ok(SampleId {
            row: row_id,
            index: sample_index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(sample_index))?,
        })
    }

    /// Number of bytes needed to represent `SampleId`.
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
            0x0C, // len = SAMPLE_ID_SIZE = 12
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            7, 0, // row index = 7
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

        assert_eq!(msg.sample_id.index, 1);
        assert_eq!(msg.sample_id.row.index, 0);
        assert_eq!(msg.sample_id.row.block_height, 1);

        let expected_ns =
            Namespace::new_v0(&[11, 13, 177, 159, 193, 156, 129, 121, 234, 136]).unwrap();
        let ns = Namespace::from_raw(&msg.share[..NS_SIZE]).unwrap();
        assert_eq!(ns, expected_ns);
    }
}
