//! Types related to rows
//!
//! Row in Celestia is understood as all the [`Share`]s in a particular
//! row of the [`ExtendedDataSquare`].
//!
//! [`Share`]: crate::Share
//! [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Row as RawRow;
use celestia_tendermint_proto::Protobuf;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};

use crate::consts::appconsts::SHARE_SIZE;
use crate::nmt::NS_SIZE;
use crate::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
use crate::rsmt2d::{is_ods_square, ExtendedDataSquare};
use crate::{DataAvailabilityHeader, Error, Result};

/// The size of the [`RowId`] hash in `multihash`.
const ROW_ID_SIZE: usize = RowId::size();
/// The code of the [`RowId`] hashing algorithm in `multihash`.
pub const ROW_ID_MULTIHASH_CODE: u64 = 0x7811;
/// The id of codec used for the [`RowId`] in `Cid`s.
pub const ROW_ID_CODEC: u64 = 0x7810;

/// Represents particular row in a specific Data Square,
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct RowId {
    block_height: u64,
    index: u16,
}

/// Row together with the data
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(try_from = "RawRow", into = "RawRow")]
pub struct Row {
    /// Location of the row in the EDS and associated block height
    pub id: RowId,
    /// Shares contained in the row
    pub shares: Vec<Vec<u8>>,
}

impl Row {
    /// Create Row with the given index from EDS
    pub fn new(index: u16, eds: &ExtendedDataSquare, block_height: u64) -> Result<Self> {
        let id = RowId::new(index, block_height)?;
        let shares = eds.row(index)?;

        Ok(Row { id, shares })
    }

    /// verify the row against roots from DAH
    pub fn verify(&self, dah: &DataAvailabilityHeader) -> Result<()> {
        let square_width =
            u16::try_from(self.shares.len()).map_err(|_| Error::EdsInvalidDimentions)?;
        let row = self.id.index;

        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));

        for col in 0..square_width {
            let share = &self.shares[usize::from(col)];

            let ns = if is_ods_square(row, col, square_width) {
                Namespace::from_raw(&share[..NS_SIZE])?
            } else {
                Namespace::PARITY_SHARE
            };

            tree.push_leaf(share, *ns).map_err(Error::Nmt)?;
        }

        let Some(root) = dah.row_root(row) else {
            return Err(Error::EdsIndexOutOfRange(row, 0));
        };

        if tree.root().hash() != root.hash() {
            return Err(Error::RootMismatch);
        }

        Ok(())
    }
}

impl Protobuf<RawRow> for Row {}

impl TryFrom<RawRow> for Row {
    type Error = Error;

    fn try_from(row: RawRow) -> Result<Row, Self::Error> {
        let id = RowId::decode(&row.row_id)?;
        let mut shares = row.row_half;
        let data_shares = shares.len();

        shares.resize(shares.len() * 2, vec![0; SHARE_SIZE]);

        leopard_codec::encode(&mut shares, data_shares)?;

        Ok(Row { id, shares })
    }
}

impl From<Row> for RawRow {
    fn from(row: Row) -> RawRow {
        let mut row_id_bytes = BytesMut::new();
        row.id.encode(&mut row_id_bytes);

        // parity shares aren't transmitted over shwap, just data shares
        let square_width = row.shares.len();
        let mut row_half = row.shares;
        row_half.truncate(square_width / 2);

        RawRow {
            row_id: row_id_bytes.to_vec(),
            row_half,
        }
    }
}

impl RowId {
    /// Create a new [`RowId`] for the particular block.
    ///
    /// # Errors
    ///
    /// This function will return an error if the block height is invalid.
    pub fn new(index: u16, block_height: u64) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        Ok(Self {
            index,
            block_height,
        })
    }

    /// Number of bytes needed to represent [`RowId`]
    pub const fn size() -> usize {
        // Size MUST be 10 by the spec.
        10
    }

    /// A height of the block which contains the data.
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// An index of the row in the [`ExtendedDataSquare`].
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn index(&self) -> u16 {
        self.index
    }

    pub(crate) fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(ROW_ID_SIZE);

        bytes.put_u64(self.block_height);
        bytes.put_u16(self.index);
    }

    pub(crate) fn decode(mut buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != ROW_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let block_height = buffer.get_u64();
        let index = buffer.get_u16();

        if block_height == 0 {
            return Err(CidError::InvalidCid("Zero block height".to_string()));
        }

        Ok(Self {
            block_height,
            index,
        })
    }
}

impl<const S: usize> TryFrom<CidGeneric<S>> for RowId {
    type Error = CidError;

    fn try_from(cid: CidGeneric<S>) -> Result<Self, Self::Error> {
        let codec = cid.codec();
        if codec != ROW_ID_CODEC {
            return Err(CidError::InvalidCidCodec(codec));
        }

        let hash = cid.hash();

        let size = hash.size() as usize;
        if size != ROW_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(size));
        }

        let code = hash.code();
        if code != ROW_ID_MULTIHASH_CODE {
            return Err(CidError::InvalidMultihashCode(code, ROW_ID_MULTIHASH_CODE));
        }

        RowId::decode(hash.digest())
    }
}

impl From<RowId> for CidGeneric<ROW_ID_SIZE> {
    fn from(row: RowId) -> Self {
        let mut bytes = BytesMut::with_capacity(ROW_ID_SIZE);
        row.encode(&mut bytes);
        // length is correct, so unwrap is safe
        let mh = Multihash::wrap(ROW_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        CidGeneric::new_v1(ROW_ID_CODEC, mh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::appconsts::SHARE_SIZE;
    use crate::test_utils::generate_eds;

    #[test]
    fn round_trip_test() {
        let row_id = RowId::new(5, 100).unwrap();
        let cid = CidGeneric::from(row_id);

        let multihash = cid.hash();
        assert_eq!(multihash.code(), ROW_ID_MULTIHASH_CODE);
        assert_eq!(multihash.size(), ROW_ID_SIZE as u8);

        let deserialized_row_id = RowId::try_from(cid).unwrap();
        assert_eq!(row_id, deserialized_row_id);
    }

    #[test]
    fn index_calculation() {
        let height = 100;
        let shares = vec![vec![0; SHARE_SIZE]; 8 * 8];
        let eds = ExtendedDataSquare::new(shares, "codec".to_string()).unwrap();

        Row::new(1, &eds, height).unwrap();
        Row::new(7, &eds, height).unwrap();
        let row_err = Row::new(8, &eds, height).unwrap_err();
        assert!(matches!(row_err, Error::EdsIndexOutOfRange(8, 0)));
        let row_err = Row::new(100, &eds, height).unwrap_err();
        assert!(matches!(row_err, Error::EdsIndexOutOfRange(100, 0)));
    }

    #[test]
    fn row_id_size() {
        assert_eq!(RowId::size(), 10);

        let row_id = RowId::new(0, 1).unwrap();
        let mut bytes = BytesMut::new();
        row_id.encode(&mut bytes);
        assert_eq!(bytes.len(), RowId::size());
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // multihash code = 7811
            0x0A, // len = ROW_ID_SIZE = 10
            0, 0, 0, 0, 0, 0, 0, 64, // block height = 64
            0, 7, // row index = 7
        ];

        let cid = CidGeneric::<ROW_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), ROW_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), ROW_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), ROW_ID_SIZE as u8);
        let row_id = RowId::try_from(cid).unwrap();
        assert_eq!(row_id.index, 7);
        assert_eq!(row_id.block_height, 64);
    }

    #[test]
    fn zero_block_height() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // code = 7811
            0x0A, // len = ROW_ID_SIZE = 10
            0, 0, 0, 0, 0, 0, 0, 0, // invalid block height = 0 !
            0, 7, // row index = 7
        ];

        let cid = CidGeneric::<ROW_ID_SIZE>::read_bytes(bytes.as_ref()).unwrap();
        assert_eq!(cid.codec(), ROW_ID_CODEC);
        let mh = cid.hash();
        assert_eq!(mh.code(), ROW_ID_MULTIHASH_CODE);
        assert_eq!(mh.size(), ROW_ID_SIZE as u8);
        let row_err = RowId::try_from(cid).unwrap_err();
        assert_eq!(
            row_err,
            CidError::InvalidCid("Zero block height".to_string())
        );
    }

    #[test]
    fn multihash_invalid_code() {
        let multihash = Multihash::<ROW_ID_SIZE>::wrap(999, &[0; ROW_ID_SIZE]).unwrap();
        let cid = CidGeneric::<ROW_ID_SIZE>::new_v1(ROW_ID_CODEC, multihash);
        let row_err = RowId::try_from(cid).unwrap_err();
        assert_eq!(
            row_err,
            CidError::InvalidMultihashCode(999, ROW_ID_MULTIHASH_CODE)
        );
    }

    #[test]
    fn cid_invalid_codec() {
        let multihash =
            Multihash::<ROW_ID_SIZE>::wrap(ROW_ID_MULTIHASH_CODE, &[0; ROW_ID_SIZE]).unwrap();
        let cid = CidGeneric::<ROW_ID_SIZE>::new_v1(1234, multihash);
        let row_err = RowId::try_from(cid).unwrap_err();
        assert_eq!(row_err, CidError::InvalidCidCodec(1234));
    }

    #[test]
    fn test_validate() {
        for _ in 0..10 {
            let eds = generate_eds(2 << (rand::random::<usize>() % 8));
            let dah = DataAvailabilityHeader::from_eds(&eds);

            let index = rand::random::<u16>() % eds.square_width();

            let row = Row {
                id: RowId {
                    block_height: 1,
                    index,
                },
                shares: eds.row(index).unwrap(),
            };

            let encoded = row.encode_vec().unwrap();
            let decoded = Row::decode(encoded.as_ref()).unwrap();

            decoded.verify(&dah).unwrap();
        }
    }
}
