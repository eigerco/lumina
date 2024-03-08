//! Types related to the namespaced data.
//!
//! Namespaced data in Celestia is understood as all the [`Share`]s within
//! the same [`Namespace`] in a single row of the [`ExtendedDataSquare`].
//!
//! [`Share`]: crate::Share
//! [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare

use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Data as RawNamespacedData;
use celestia_tendermint_proto::Protobuf;
use cid::CidGeneric;
use multihash::Multihash;
use serde::{Deserialize, Serialize};

use crate::nmt::{Namespace, NamespaceProof};
use crate::row::{RowId, ROW_ID_SIZE};
use crate::{DataAvailabilityHeader, Error, Result};

/// Number of bytes needed to represent [`NamespacedDataId`] in `multihash`.
const NAMESPACED_DATA_ID_SIZE: usize = 39;
/// The code of the [`NamespacedDataId`] hashing algorithm in `multihash`.
pub const NAMESPACED_DATA_ID_MULTIHASH_CODE: u64 = 0x7821;
/// The id of codec used for the [`NamespacedDataId`] in `Cid`s.
pub const NAMESPACED_DATA_ID_CODEC: u64 = 0x7820;

/// Identifies [`Share`]s within a [`Namespace`] located on a particular row of the
/// block's [`ExtendedDataSquare`].
///
/// [`Share`]: crate::Share
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct NamespacedDataId {
    row_id: RowId,
    namespace: Namespace,
}

/// `NamespacedData` contains up to a row of shares belonging to a particular namespace and a proof of their inclusion.
///
/// It is constructed out of the ExtendedDataSquare. If, for particular EDS, shares from the namespace span multiple rows,
/// one needs multiple NamespacedData instances to cover the whole range.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(try_from = "RawNamespacedData", into = "RawNamespacedData")]
pub struct NamespacedData {
    /// Location of the shares on EDS
    pub id: NamespacedDataId,
    /// Proof of data inclusion
    pub proof: NamespaceProof,
    /// Shares with data
    pub shares: Vec<Vec<u8>>,
}

impl NamespacedData {
    /// Verifies proof inside `NamespacedData` using a row root from [`DataAvailabilityHeader`]
    ///
    /// #Example
    /// ```no_run
    /// use celestia_types::nmt::Namespace;
    /// # use celestia_types::{ExtendedDataSquare, ExtendedHeader};
    /// # fn get_extended_data_square(height: usize) -> ExtendedDataSquare {
    /// #    unimplemented!()
    /// # }
    /// # fn get_extended_header(height: usize) -> ExtendedHeader {
    /// #    unimplemented!()
    /// # }
    /// #
    /// let block_height = 100;
    /// let eds = get_extended_data_square(block_height);
    /// let header = get_extended_header(block_height);
    ///
    /// let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    ///
    /// let rows = eds.get_namespaced_data(namespace, &header.dah, block_height as u64).unwrap();
    /// for namespaced_data in rows {
    ///     namespaced_data.verify(&header.dah).unwrap()
    /// }
    /// ```
    ///
    /// [`DataAvailabilityHeader`]: crate::DataAvailabilityHeader
    pub fn verify(&self, dah: &DataAvailabilityHeader) -> Result<()> {
        if self.shares.is_empty() {
            return Err(Error::WrongProofType);
        }

        let namespace = self.id.namespace();
        let row = self.id.row_index();
        let root = dah.row_root(row).ok_or(Error::EdsIndexOutOfRange(row, 0))?;

        self.proof
            .verify_complete_namespace(&root, &self.shares, *namespace)
            .map_err(Error::RangeProofError)
    }
}

impl Protobuf<RawNamespacedData> for NamespacedData {}

impl TryFrom<RawNamespacedData> for NamespacedData {
    type Error = Error;

    fn try_from(namespaced_data: RawNamespacedData) -> Result<NamespacedData, Self::Error> {
        let Some(proof) = namespaced_data.data_proof else {
            return Err(Error::MissingProof);
        };

        let id = NamespacedDataId::decode(&namespaced_data.data_id)?;

        Ok(NamespacedData {
            id,
            shares: namespaced_data.data_shares,
            proof: proof.try_into()?,
        })
    }
}

impl From<NamespacedData> for RawNamespacedData {
    fn from(namespaced_data: NamespacedData) -> RawNamespacedData {
        let mut data_id_bytes = BytesMut::new();
        namespaced_data.id.encode(&mut data_id_bytes);

        RawNamespacedData {
            data_id: data_id_bytes.to_vec(),
            data_shares: namespaced_data.shares.iter().map(|s| s.to_vec()).collect(),
            data_proof: Some(namespaced_data.proof.into()),
        }
    }
}

impl NamespacedDataId {
    /// Create a new [`NamespacedDataId`] for given block, row and the [`Namespace`].
    ///
    /// # Errors
    ///
    /// This function will return an error if the block height
    /// or row index is invalid.
    pub fn new(namespace: Namespace, row_index: u16, block_height: u64) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        Ok(Self {
            row_id: RowId::new(row_index, block_height)?,
            namespace,
        })
    }

    /// A height of the block which contains the shares.
    pub fn block_height(&self) -> u64 {
        self.row_id.block_height()
    }

    /// Row index of the [`ExtendedDataSquare`] that shares are located on.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn row_index(&self) -> u16 {
        self.row_id.index()
    }

    /// A namespace of the [`Share`]s.
    ///
    /// [`Share`]: crate::Share
    pub fn namespace(&self) -> Namespace {
        self.namespace
    }

    fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(NAMESPACED_DATA_ID_SIZE);
        self.row_id.encode(bytes);
        bytes.put(self.namespace.as_bytes());
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != NAMESPACED_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (row_bytes, ns_bytes) = buffer.split_at(ROW_ID_SIZE);
        let row_id = RowId::decode(row_bytes)?;
        let namespace =
            Namespace::from_raw(ns_bytes).map_err(|e| CidError::InvalidCid(e.to_string()))?;

        Ok(Self { row_id, namespace })
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

impl From<NamespacedDataId> for CidGeneric<NAMESPACED_DATA_ID_SIZE> {
    fn from(namespaced_data_id: NamespacedDataId) -> Self {
        let mut bytes = BytesMut::with_capacity(NAMESPACED_DATA_ID_SIZE);
        namespaced_data_id.encode(&mut bytes);
        // length is correct, so the unwrap is safe
        let mh = Multihash::wrap(NAMESPACED_DATA_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        CidGeneric::new_v1(NAMESPACED_DATA_ID_CODEC, mh)
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
        let cid = CidGeneric::from(data_id);

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
            0, 0, 0, 0, 0, 0, 0, 64, // block height = 64
            0, 7, // row = 7
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // NS = 1
        ];

        let cid = CidGeneric::<NAMESPACED_DATA_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), NAMESPACED_DATA_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), NAMESPACED_DATA_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), NAMESPACED_DATA_ID_SIZE as u8);
        let data_id = NamespacedDataId::try_from(cid).unwrap();
        assert_eq!(data_id.namespace(), Namespace::new_v0(&[1]).unwrap());
        assert_eq!(data_id.block_height(), 64);
        assert_eq!(data_id.row_index(), 7);
    }

    #[test]
    fn namespaced_data_id_size() {
        // Size MUST be 39 by the spec.
        assert_eq!(NAMESPACED_DATA_ID_SIZE, 39);

        let data_id = NamespacedDataId::new(Namespace::new_v0(&[1]).unwrap(), 0, 1).unwrap();
        let mut bytes = BytesMut::new();
        data_id.encode(&mut bytes);
        assert_eq!(bytes.len(), NAMESPACED_DATA_ID_SIZE);
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
        assert_eq!(msg.id.namespace(), ns);
        assert_eq!(msg.id.row_index(), 0);
        assert_eq!(msg.id.block_height(), 1);

        for s in msg.shares {
            let s = Share::from_raw(&s).unwrap();
            assert_eq!(s.namespace(), ns);
        }
    }
}
