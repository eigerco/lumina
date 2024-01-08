use std::io::Cursor;

use blockstore::block::CidError;
use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::share::p2p::shwap::Row as RawRow;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};
use tendermint::Hash;
use tendermint_proto::Protobuf;

use crate::nmt::{NamespacedSha2Hasher, Nmt};
use crate::rsmt2d::ExtendedDataSquare;
use crate::{Error, Result, Share};

const ROW_ID_SIZE: usize = RowId::size();
pub const ROW_ID_MULTIHASH_CODE: u64 = 0x7811;
pub const ROW_ID_CODEC: u64 = 0x7810;

/// Represents particular particular Row in a specific Data Square,
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct RowId {
    ///
    pub block_height: u64,
    pub index: u16,
}

/// Row together with the data
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(try_from = "RawRow", into = "RawRow")]
pub struct Row {
    /// Location of the row in the EDS and associated block height
    pub row_id: RowId,

    /// Shares contained in the row
    pub shares: Vec<Share>,
}

impl Row {
    /// Create Row with given index from the EDS
    pub fn new(index: usize, eds: &ExtendedDataSquare, block_height: u64) -> Result<Self> {
        let square_len = eds.square_len();

        let row_id = RowId::new(index, block_height)?;
        let mut shares = eds.row(index)?;
        shares.truncate(square_len / 2);

        Ok(Row { row_id, shares })
    }

    pub fn validate(&self, root_hash: Hash) -> Result<()> {
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
            tree.push_leaf(s.data(), *Namespace::PARITY_SHARE)
                .map_err(Error::Nmt)?;
        }
        */

        if tree.root().hash() != root_hash.as_ref() {
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
        let shares = row
            .row_half
            .into_iter()
            .map(|s| Share::from_raw(&s))
            .collect::<Result<Vec<_>>>()?;

        Ok(Row { row_id, shares })
    }
}

impl From<Row> for RawRow {
    fn from(row: Row) -> RawRow {
        let mut row_id_bytes = BytesMut::new();
        row.row_id.encode(&mut row_id_bytes);

        RawRow {
            row_id: row_id_bytes.to_vec(),
            row_half: row.shares.into_iter().map(|s| s.data.to_vec()).collect(),
        }
    }
}

impl RowId {
    /// Create new axis for the particular data square
    pub fn new(index: usize, block_height: u64) -> Result<Self> {
        if block_height == 0 {
            return Err(Error::ZeroBlockHeight);
        }

        Ok(Self {
            index: index
                .try_into()
                .map_err(|_| Error::EdsIndexOutOfRange(index))?,
            block_height,
        })
    }

    /// Number of bytes needed to represent `RowId`
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
        let shares = vec![Share::from_raw(&[0; SHARE_SIZE]).unwrap(); 8 * 8];
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
    fn decode_row_bytes() {
        let bytes = include_bytes!("../test_data/shwap_samples/row.data");
        let mut row = Row::decode(&bytes[..]).unwrap();

        row.row_id.index = 64;
        row.row_id.block_height = 255;

        assert_eq!(row.row_id.index, 64);
        assert_eq!(row.row_id.block_height, 255);

        for (idx, share) in row.shares.iter().enumerate() {
            let ns = Namespace::new_v0(&[idx as u8]).unwrap();
            assert_eq!(share.namespace(), ns);
            let data = [0; SHARE_SIZE - NS_SIZE];
            assert_eq!(share.data(), data);
        }
    }
}
