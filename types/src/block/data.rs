use celestia_proto::tendermint_celestia_mods::types::Data as RawData;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::Error;

/// Data contained in a [`Block`].
///
/// [`Block`]: crate::block::Block
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "RawData", into = "RawData")]
pub struct Data {
    /// Transactions.
    pub txs: Vec<Vec<u8>>,

    /// Square width of original data square.
    pub square_size: u64,

    /// Hash is the root of a binary Merkle tree where the leaves of the tree are
    /// the row and column roots of an extended data square. Hash is often referred
    /// to as the "data root".
    pub hash: Vec<u8>,
}

impl Protobuf<RawData> for Data {}

impl TryFrom<RawData> for Data {
    type Error = Error;

    fn try_from(value: RawData) -> Result<Self, Self::Error> {
        Ok(Data {
            txs: value.txs,
            square_size: value.square_size,
            hash: value.hash,
        })
    }
}

impl From<Data> for RawData {
    fn from(value: Data) -> RawData {
        RawData {
            txs: value.txs,
            square_size: value.square_size,
            hash: value.hash,
        }
    }
}

#[cfg(feature = "uniffi")]
mod uniffi_types {
    use super::Data as BlockData;

    // we need to rename `Data`, otherwise it clashes with `Foundation::Data` for Swift
    #[uniffi::remote(Record)]
    pub struct BlockData {
        /// Transactions.
        pub txs: Vec<Vec<u8>>,

        /// Square width of original data square.
        pub square_size: u64,

        /// Hash is the root of a binary Merkle tree where the leaves of the tree are
        /// the row and column roots of an extended data square. Hash is often referred
        /// to as the "data root".
        pub hash: Vec<u8>,
    }
}
