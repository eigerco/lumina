use std::io::Cursor;

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Row as RawRow;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::nmt::NS_SIZE;
use crate::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
use crate::rsmt2d::ExtendedDataSquare;
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
    /// A height of the block which contains the data.
    pub block_height: u64,
    /// An index of the row in the [`ExtendedDataSquare`].
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub index: u16,
}

/// Row together with the data
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(try_from = "RawRow", into = "RawRow")]
pub struct Row {
    /// Location of the row in the EDS and associated block height
    pub row_id: RowId,
    /// Shares contained in the row
    pub shares: Vec<Vec<u8>>,
}

impl Row {
    /// Create Row with the given index from EDS
    pub fn new(index: u16, eds: &ExtendedDataSquare, block_height: u64) -> Result<Self> {
        let square_len = eds.square_len();

        let row_id = RowId::new(index, block_height)?;
        let mut shares = eds.row(index.into())?;
        shares.truncate(square_len / 2);

        Ok(Row { row_id, shares })
    }

    /// Validate the row against roots from DAH
    pub fn validate(&self, dah: &DataAvailabilityHeader) -> Result<()> {
        let square_len = self.shares.len();

        let (data_shares, parity_shares) = self.shares.split_at(square_len / 2);

        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
        for s in data_shares {
            let ns = Namespace::from_raw(&s[..NS_SIZE])?;
            tree.push_leaf(s, *ns).map_err(Error::Nmt)?;
        }
        for s in parity_shares {
            tree.push_leaf(s, *Namespace::PARITY_SHARE)
                .map_err(Error::Nmt)?;
        }

        let index = self.row_id.index.into();
        let Some(root) = dah.row_root(index) else {
            return Err(Error::EdsIndexOutOfRange(index));
        };

        if tree.root().hash() != root.hash() {
            return Err(Error::RootMismatch);
        }

        unimplemented!("unable to compute parity shares")
    }
}

impl Protobuf<RawRow> for Row {}

impl TryFrom<RawRow> for Row {
    type Error = Error;

    fn try_from(row: RawRow) -> Result<Row, Self::Error> {
        let row_id = RowId::decode(&row.row_id)?;
        let shares = row.row_half;

        // TODO: only original data shares are sent over the wire, we need leopard codec to
        // re-compute parity shares
        //
        // somehow_generate_parity_shares(&mut shares);

        let _ = Row { row_id, shares };

        unimplemented!()
    }
}

impl From<Row> for RawRow {
    fn from(row: Row) -> RawRow {
        let mut row_id_bytes = BytesMut::new();
        row.row_id.encode(&mut row_id_bytes);

        // parity shares aren't transmitted over shwap, just data shares
        let square_len = row.shares.len();
        let mut row_half = row.shares;
        row_half.truncate(square_len / 2);

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
        // size of:
        // u16 + u64
        // 2  + 8
        10
    }

    pub(crate) fn encode(&self, bytes: &mut BytesMut) {
        bytes.reserve(ROW_ID_SIZE);

        bytes.put_u64_le(self.block_height);
        bytes.put_u16_le(self.index);
    }

    pub(crate) fn decode(buffer: &[u8]) -> Result<Self, CidError> {
        if buffer.len() != ROW_ID_SIZE {
            return Err(CidError::InvalidMultihashLength(buffer.len()));
        }

        let mut cursor = Cursor::new(buffer);
        let block_height = cursor.get_u64_le();
        let index = cursor.get_u16_le();

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

impl TryFrom<RowId> for CidGeneric<ROW_ID_SIZE> {
    type Error = CidError;

    fn try_from(row: RowId) -> Result<Self, Self::Error> {
        let mut bytes = BytesMut::with_capacity(ROW_ID_SIZE);
        row.encode(&mut bytes);
        // length is correct, so unwrap is safe
        let mh = Multihash::wrap(ROW_ID_MULTIHASH_CODE, &bytes[..]).unwrap();

        Ok(CidGeneric::new_v1(ROW_ID_CODEC, mh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::appconsts::SHARE_SIZE;
    use crate::nmt::{Namespace, NS_SIZE};

    #[test]
    fn round_trip_test() {
        let row_id = RowId::new(5, 100).unwrap();
        let cid = CidGeneric::try_from(row_id).unwrap();

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
        assert!(matches!(row_err, Error::EdsIndexOutOfRange(8)));
        let row_err = Row::new(100, &eds, height).unwrap_err();
        assert!(matches!(row_err, Error::EdsIndexOutOfRange(100)));
    }

    #[test]
    fn from_buffer() {
        let bytes = [
            0x01, // CIDv1
            0x90, 0xF0, 0x01, // CID codec = 7810
            0x91, 0xF0, 0x01, // multihash code = 7811
            0x0A, // len = ROW_ID_SIZE = 10
            64, 0, 0, 0, 0, 0, 0, 0, // block height = 64
            7, 0, // row index = 7
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
            7, 0, // row index = 7
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
    // TODO:
    // fully testing protobuf deserialisation requires leopard codec, to generate parity shares.
    // By asserting that we've reached `unimplemented!`, we rely on implementation detail to
    // check whether protobuf, RowId and Share deserialisations were successful (but can't check
    // the actual data)
    // Once we have leopard codec, remove `should_panic` to enable full test functionality
    #[should_panic(expected = "not implemented")]
    fn decode_row_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/row.data");
        let mut row = Row::decode(&bytes[..]).unwrap();

        row.row_id.index = 64;
        row.row_id.block_height = 255;

        assert_eq!(row.row_id.index, 64);
        assert_eq!(row.row_id.block_height, 255);

        for (idx, share) in row.shares.iter().enumerate() {
            let expected_ns = Namespace::new_v0(&[idx as u8]).unwrap();
            let ns = Namespace::from_raw(&share[..NS_SIZE]).unwrap();
            assert_eq!(ns, expected_ns);
            let data = [0; SHARE_SIZE - NS_SIZE];
            assert_eq!(share[NS_SIZE..], data);
        }
    }
}
