//! Blocks within the chains of a Tendermint network

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use crate::evidence::JsEvidence;
use crate::{Error, Result};
use celestia_proto::tendermint_celestia_mods::types::Block as RawBlock;
use serde::{Deserialize, Serialize};
use tendermint::block::{Commit, Header};
use tendermint::evidence;
use tendermint_proto::Protobuf;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::prelude::*;

mod commit;
mod data;
pub(crate) mod header;

pub use commit::CommitExt;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use commit::{JsCommit, JsCommitSig};
pub use data::Data;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use header::{JsHeader, JsHeaderVersion, JsPartsHeader, JsSignedHeader};

pub(crate) const GENESIS_HEIGHT: u64 = 1;

/// The height of the block in Celestia network.
pub type Height = tendermint::block::Height;

/// Blocks consist of a header, transactions, votes (the commit), and a list of
/// evidence of malfeasance (i.e. signing conflicting votes).
///
/// This is a modified version of [`tendermint::block::Block`] which contains
/// [modifications](data-mod) that Celestia introduced.
///
/// [data-mod]: https://github.com/celestiaorg/celestia-core/blob/a1268f7ae3e688144a613c8a439dd31818aae07d/proto/tendermint/types/types.proto#L84-L104
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "RawBlock", into = "RawBlock")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
    wasm_bindgen(getter_with_clone)
)]
pub struct Block {
    /// Block header
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub header: Header,

    /// Transaction data
    pub data: Data,

    /// Evidence of malfeasance
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub evidence: evidence::List,

    /// Last commit, should be `None` for the initial block.
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm-bindgen"),
        wasm_bindgen(skip)
    )]
    pub last_commit: Option<Commit>,
}

impl Block {
    /// Builds a new [`Block`], based on the given [`Header`], [`Data`], evidence, and last commit.
    pub fn new(
        header: Header,
        data: Data,
        evidence: evidence::List,
        last_commit: Option<Commit>,
    ) -> Self {
        Block {
            header,
            data,
            evidence,
            last_commit,
        }
    }

    /// Get header
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Get data
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// Get evidence
    pub fn evidence(&self) -> &evidence::List {
        &self.evidence
    }

    /// Get last commit
    pub fn last_commit(&self) -> &Option<Commit> {
        &self.last_commit
    }
}

impl Protobuf<RawBlock> for Block {}

impl TryFrom<RawBlock> for Block {
    type Error = Error;

    fn try_from(value: RawBlock) -> Result<Self, Self::Error> {
        let header: Header = value
            .header
            .ok_or_else(tendermint::Error::missing_header)?
            .try_into()?;

        // If last_commit is the default Commit, it is considered nil by Go.
        let last_commit = value
            .last_commit
            .map(TryInto::try_into)
            .transpose()?
            .filter(|c| c != &Commit::default());

        Ok(Block::new(
            header,
            value
                .data
                .ok_or_else(tendermint::Error::missing_data)?
                .try_into()?,
            value
                .evidence
                .map(TryInto::try_into)
                .transpose()?
                .unwrap_or_default(),
            last_commit,
        ))
    }
}

impl From<Block> for RawBlock {
    fn from(value: Block) -> Self {
        RawBlock {
            header: Some(value.header.into()),
            data: Some(value.data.into()),
            evidence: Some(value.evidence.into()),
            last_commit: value.last_commit.map(Into::into),
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
#[wasm_bindgen]
impl Block {
    /// Block header
    #[wasm_bindgen(getter)]
    pub fn get_header(&self) -> JsHeader {
        self.header.clone().into()
    }

    /// Evidence of malfeasance
    #[wasm_bindgen(getter)]
    pub fn get_evidence(&self) -> Vec<JsEvidence> {
        self.evidence
            .iter()
            .map(|e| JsEvidence::from(e.clone()))
            .collect()
    }

    /// Last commit, should be `None` for the initial block.
    #[wasm_bindgen(getter)]
    pub fn get_last_commit(&self) -> Option<JsCommit> {
        self.last_commit.clone().map(Into::into)
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use tendermint::block;
    use wasm_bindgen::prelude::*;

    use crate::block::header::JsPartsHeader;

    /// Block identifiers which contain two distinct Merkle roots of the block, as well as the number of parts in the block.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "BlockId")]
    pub struct JsBlockId {
        /// The block’s main hash is the Merkle root of all the fields in the block header.
        pub hash: String,
        /// Parts header (if available) is used for secure gossipping of the block during
        /// consensus. It is the Merkle root of the complete serialized block cut into parts.
        ///
        /// PartSet is used to split a byteslice of data into parts (pieces) for transmission.
        /// By splitting data into smaller parts and computing a Merkle root hash on the list,
        /// you can verify that a part is legitimately part of the complete data, and the part
        /// can be forwarded to other peers before all the parts are known. In short, it’s
        /// a fast way to propagate a large file over a gossip network.
        ///
        /// <https://github.com/tendermint/tendermint/wiki/Block-Structure#partset>
        ///
        /// PartSetHeader in protobuf is defined as never nil using the gogoproto annotations.
        /// This does not translate to Rust, but we can indicate this in the domain type.
        pub part_set_header: JsPartsHeader,
    }

    impl From<block::Id> for JsBlockId {
        fn from(value: block::Id) -> Self {
            JsBlockId {
                hash: value.hash.to_string(),
                part_set_header: value.part_set_header.into(),
            }
        }
    }
}

#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::block::Height as TendermintHeight;
    use tendermint::block::Id as TendermintBlockId;
    use tendermint::hash::Hash;
    use uniffi::Record;

    use crate::block::header::uniffi_types::PartsHeader;
    use crate::error::UniffiConversionError;

    /// Block identifiers which contain two distinct Merkle roots of the block, as well as the number of parts in the block.
    #[derive(Record)]
    pub struct BlockId {
        /// The block’s main hash is the Merkle root of all the fields in the block header.
        pub hash: Hash,
        /// Parts header (if available) is used for secure gossipping of the block during
        /// consensus. It is the Merkle root of the complete serialized block cut into parts.
        ///
        /// PartSet is used to split a byteslice of data into parts (pieces) for transmission.
        /// By splitting data into smaller parts and computing a Merkle root hash on the list,
        /// you can verify that a part is legitimately part of the complete data, and the part
        /// can be forwarded to other peers before all the parts are known. In short, it’s
        /// a fast way to propagate a large file over a gossip network.
        ///
        /// <https://github.com/tendermint/tendermint/wiki/Block-Structure#partset>
        ///
        /// PartSetHeader in protobuf is defined as never nil using the gogoproto annotations.
        /// This does not translate to Rust, but we can indicate this in the domain type.
        pub part_set_header: PartsHeader,
    }

    impl TryFrom<BlockId> for TendermintBlockId {
        type Error = UniffiConversionError;

        fn try_from(value: BlockId) -> Result<Self, Self::Error> {
            Ok(TendermintBlockId {
                hash: value.hash,
                part_set_header: value.part_set_header.try_into()?,
            })
        }
    }

    impl From<TendermintBlockId> for BlockId {
        fn from(value: TendermintBlockId) -> Self {
            BlockId {
                hash: value.hash,
                part_set_header: value.part_set_header.into(),
            }
        }
    }

    uniffi::custom_type!(TendermintBlockId, BlockId, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });

    /// Block height for a particular chain (i.e. number of blocks created since the chain began)
    ///
    /// A height of 0 represents a chain which has not yet produced a block.
    #[derive(Record)]
    pub struct BlockHeight {
        /// Height value
        pub value: u64,
    }

    impl TryFrom<BlockHeight> for TendermintHeight {
        type Error = UniffiConversionError;

        fn try_from(value: BlockHeight) -> Result<Self, Self::Error> {
            TendermintHeight::try_from(value.value)
                .map_err(|_| UniffiConversionError::HeaderHeightOutOfRange)
        }
    }

    impl From<TendermintHeight> for BlockHeight {
        fn from(value: TendermintHeight) -> Self {
            BlockHeight {
                value: value.value(),
            }
        }
    }

    uniffi::custom_type!(TendermintHeight, BlockHeight, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });
}
