use celestia_proto::tendermint_celestia_mods::types::Data as RawData;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::prelude::*;

use crate::Error;

/// Data contained in a [`Block`].
///
/// [`Block`]: crate::block::Block
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "RawData", into = "RawData")]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
    wasm_bindgen(getter_with_clone)
)]
pub struct Data {
    /// Transactions.
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub txs: Vec<Vec<u8>>,

    /// Square width of original data square.
    pub square_size: u64,

    /// Hash is the root of a binary Merkle tree where the leaves of the tree are
    /// the row and column roots of an extended data square. Hash is often referred
    /// to as the "data root".
    pub hash: Vec<u8>,
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
#[wasm_bindgen]
impl Data {
    /// Transactions
    #[wasm_bindgen(getter)]
    pub fn transactions(&self) -> Vec<Uint8Array> {
        self.txs
            .iter()
            .map(|tx| Uint8Array::from(&tx[..]))
            .collect()
    }
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
