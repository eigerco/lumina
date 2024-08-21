//! Types related to the namespaced data.
//!
//! Namespaced data in Celestia is understood as all the [`Share`]s within
//! the same [`Namespace`] in a single row of the [`ExtendedDataSquare`].
//!
//! [`Share`]: crate::Share
//! [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare

use blockstore::block::CidError;
use bytes::{BufMut, BytesMut};
use celestia_proto::shwap::{RowNamespaceData as RawRowNamespaceData, Share as RawShare};
use celestia_tendermint_proto::Protobuf;
use cid::CidGeneric;
use multihash::Multihash;
use serde::{Deserialize, Serialize};

use crate::nmt::{Namespace, NamespaceProof};
use crate::row::{RowId, ROW_ID_SIZE};
use crate::{DataAvailabilityHeader, Error, Result};

/// Number of bytes needed to represent [`RowNamespaceDataId`] in `multihash`.
const ROW_NAMESPACE_DATA_ID_SIZE: usize = 39;
/// The code of the [`RowNamespaceDataId`] hashing algorithm in `multihash`.
pub const ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE: u64 = 0x7821;
/// The id of codec used for the [`RowNamespaceDataId`] in `Cid`s.
pub const ROW_NAMESPACE_DATA_CODEC: u64 = 0x7820;

/// Identifies [`Share`]s within a [`Namespace`] located on a particular row of the
/// block's [`ExtendedDataSquare`].
///
/// [`Share`]: crate::Share
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct RowNamespaceDataId {
    row_id: RowId,
    namespace: Namespace,
}

/// `RowNamespaceData` contains up to a row of shares belonging to a particular namespace and a proof of their inclusion.
///
/// It is constructed out of the ExtendedDataSquare. If, for particular EDS, shares from the namespace span multiple rows,
/// one needs multiple RowNamespaceData instances to cover the whole range.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(try_from = "RawRowNamespaceData", into = "RawRowNamespaceData")]
pub struct RowNamespaceData {
    /// Proof of data inclusion
    pub proof: NamespaceProof,
    /// Shares with data
    pub shares: Vec<Vec<u8>>,
}

impl RowNamespaceData {
    /// Verifies proof inside `RowNamespaceData` using a row root from [`DataAvailabilityHeader`]
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
    pub fn verify(&self, id: RowNamespaceDataId, dah: &DataAvailabilityHeader) -> Result<()> {
        if self.shares.is_empty() {
            return Err(Error::WrongProofType);
        }

        let namespace = id.namespace();
        let row = id.row_index();
        let root = dah.row_root(row).ok_or(Error::EdsIndexOutOfRange(row, 0))?;

        self.proof
            .verify_complete_namespace(&root, &self.shares, *namespace)
            .map_err(Error::RangeProofError)
    }
}

impl Protobuf<RawRowNamespaceData> for RowNamespaceData {}

impl TryFrom<RawRowNamespaceData> for RowNamespaceData {
    type Error = Error;

    fn try_from(namespaced_data: RawRowNamespaceData) -> Result<RowNamespaceData, Self::Error> {
        let Some(proof) = namespaced_data.proof else {
            return Err(Error::MissingProof);
        };

        Ok(RowNamespaceData {
            shares: namespaced_data
                .shares
                .into_iter()
                .map(|shr| shr.data)
                .collect(),
            proof: proof.try_into()?,
        })
    }
}

impl From<RowNamespaceData> for RawRowNamespaceData {
    fn from(namespaced_data: RowNamespaceData) -> RawRowNamespaceData {
        RawRowNamespaceData {
            shares: namespaced_data
                .shares
                .into_iter()
                .map(|data| RawShare { data })
                .collect(),
            proof: Some(namespaced_data.proof.into()),
        }
    }
}

impl RowNamespaceDataId {
    /// Create a new [`RowNamespaceDataId`] for given block, row and the [`Namespace`].
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
        bytes.reserve(ROW_NAMESPACE_DATA_ID_SIZE);
        self.row_id.encode(bytes);
        bytes.put(self.namespace.as_bytes());
    }

    fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != ROW_NAMESPACE_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let (row_bytes, ns_bytes) = buffer.split_at(ROW_ID_SIZE);
        let row_id = RowId::decode(row_bytes)?;
        let namespace =
            Namespace::from_raw(ns_bytes).map_err(|e| CidError::InvalidCid(e.to_string()))?;

        Ok(Self { row_id, namespace })
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for RowNamespaceDataId {
    type Error = CidError;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != ROW_NAMESPACE_DATA_CODEC {
            return Err(CidError::InvalidCidCodec(codec));
        }

        let hash = cid.hash();

        let size = hash.size() as usize;
        if size != ROW_NAMESPACE_DATA_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE {
            return Err(CidError::InvalidMultihashCode(
                code,
                ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE,
            ));
        }

        RowNamespaceDataId::decode(hash.digest())
    }
}

impl From<RowNamespaceDataId> for CidGeneric<ROW_NAMESPACE_DATA_ID_SIZE> {
    fn from(namespaced_data_id: RowNamespaceDataId) -> Self {
        let mut bytes = BytesMut::with_capacity(ROW_NAMESPACE_DATA_ID_SIZE);
        namespaced_data_id.encode(&mut bytes);
        // length is correct, so the unwrap is safe
        let mh = Multihash::wrap(ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        CidGeneric::new_v1(ROW_NAMESPACE_DATA_CODEC, mh)
    }
}

#[cfg(test)]
mod tests {
    use crate::nmt::NS_SIZE;

    use super::*;

    #[test]
    fn round_trip() {
        let ns = Namespace::new_v0(&[0, 1]).unwrap();
        let data_id = RowNamespaceDataId::new(ns, 5, 100).unwrap();
        let cid = CidGeneric::from(data_id);

        let multihash = cid.hash();
        assert_eq!(multihash.code(), ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), ROW_NAMESPACE_DATA_ID_SIZE as u8);

        let deserialized_data_id = RowNamespaceDataId::try_from(cid).unwrap();
        assert_eq!(data_id, deserialized_data_id);
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0xA0, 0xF0, 0x01, // CID codec = 7820
            0xA1, 0xF0, 0x01, // multihash code = 7821
            0x27, // len = ROW_NAMESPACE_DATA_ID_SIZE = 39
            0, 0, 0, 0, 0, 0, 0, 64, // block height = 64
            0, 7, // row = 7
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // NS = 1
        ];

        let cid = CidGeneric::<ROW_NAMESPACE_DATA_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), ROW_NAMESPACE_DATA_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), ROW_NAMESPACE_DATA_ID_SIZE as u8);
        let data_id = RowNamespaceDataId::try_from(cid).unwrap();
        assert_eq!(data_id.namespace(), Namespace::new_v0(&[1]).unwrap());
        assert_eq!(data_id.block_height(), 64);
        assert_eq!(data_id.row_index(), 7);
    }

    #[test]
    fn namespaced_data_id_size() {
        // Size MUST be 39 by the spec.
        assert_eq!(ROW_NAMESPACE_DATA_ID_SIZE, 39);

        let data_id = RowNamespaceDataId::new(Namespace::new_v0(&[1]).unwrap(), 0, 1).unwrap();
        let mut bytes = BytesMut::new();
        data_id.encode(&mut bytes);
        assert_eq!(bytes.len(), ROW_NAMESPACE_DATA_ID_SIZE);
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash =
            Multihash::<ROW_NAMESPACE_DATA_ID_SIZE>::wrap(888, &[0; ROW_NAMESPACE_DATA_ID_SIZE])
                .unwrap();
        let cid =
            CidGeneric::<ROW_NAMESPACE_DATA_ID_SIZE>::new_v1(ROW_NAMESPACE_DATA_CODEC, multihash);
        let axis_err = RowNamespaceDataId::try_from(cid).unwrap_err();
        assert_eq!(
            axis_err,
            CidError::InvalidMultihashCode(888, ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE)
        );
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash = Multihash::<ROW_NAMESPACE_DATA_ID_SIZE>::wrap(
            ROW_NAMESPACE_DATA_ID_MULTIHASH_CODE,
            &[0; ROW_NAMESPACE_DATA_ID_SIZE],
        )
        .unwrap();
        let cid = CidGeneric::<ROW_NAMESPACE_DATA_ID_SIZE>::new_v1(4321, multihash);
        let axis_err = RowNamespaceDataId::try_from(cid).unwrap_err();
        assert_eq!(axis_err, CidError::InvalidCidCodec(4321));
    }

    #[test]
    fn decode_data_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/namespaced_data.data");
        let (id, msg) = bytes.split_at(ROW_NAMESPACE_DATA_ID_SIZE);
        let id = RowNamespaceDataId::decode(id).unwrap();
        let msg = RowNamespaceData::decode(msg).unwrap();

        let ns = Namespace::new_v0(&[135, 30, 47, 81, 60, 66, 177, 20, 57, 85]).unwrap();
        assert_eq!(id.namespace(), ns);
        assert_eq!(id.row_index(), 0);
        assert_eq!(id.block_height(), 1);

        for s in msg.shares {
            assert_eq!(&s[..NS_SIZE], ns.as_ref());
        }
    }
}
