//! Blocks within the chains of a Tendermint network

use celestia_proto::tendermint_celestia_mods::types::Block as RawBlock;
use serde::{Deserialize, Serialize};
use tendermint::block::{Commit, CommitSig, Header, Id};
use tendermint::signature::SIGNATURE_LENGTH;
use tendermint::{chain, evidence, vote, Vote};
use tendermint_proto::Protobuf;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::prelude::*;

use crate::consts::{genesis::MAX_CHAIN_ID_LEN, version};
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use crate::evidence::JsEvidence;
use crate::hash::Hash;
use crate::{bail_validation, Error, Result, ValidateBasic, ValidationError};

mod data;

pub use data::Data;

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

impl ValidateBasic for Header {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.version.block != version::BLOCK_PROTOCOL {
            bail_validation!(
                "version block ({}) != block protocol ({})",
                self.version.block,
                version::BLOCK_PROTOCOL,
            )
        }

        if self.chain_id.as_str().len() > MAX_CHAIN_ID_LEN {
            bail_validation!(
                "chain id ({}) len > maximum ({})",
                self.chain_id,
                MAX_CHAIN_ID_LEN
            )
        }

        if self.height.value() == 0 {
            bail_validation!("height == 0")
        }

        if self.height.value() == GENESIS_HEIGHT && self.last_block_id.is_some() {
            bail_validation!("last_block_id == Some() at height {GENESIS_HEIGHT}");
        }

        if self.height.value() != GENESIS_HEIGHT && self.last_block_id.is_none() {
            bail_validation!("last_block_id == None at height {}", self.height)
        }

        // NOTE: We do not validate `Hash` fields because they are type safe.
        // In Go implementation the validation passes if their length is 0 or 32.
        //
        // NOTE: We do not validate `app_hash` because if can be anything

        Ok(())
    }
}

impl ValidateBasic for Commit {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.height.value() >= GENESIS_HEIGHT {
            if is_zero(&self.block_id) {
                bail_validation!("block_id is zero")
            }

            if self.signatures.is_empty() {
                bail_validation!("no signatures in commit")
            }

            for commit_sig in &self.signatures {
                commit_sig.validate_basic()?;
            }
        }
        Ok(())
    }
}

impl ValidateBasic for CommitSig {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        match self {
            CommitSig::BlockIdFlagAbsent => (),
            CommitSig::BlockIdFlagCommit { signature, .. }
            | CommitSig::BlockIdFlagNil { signature, .. } => {
                if let Some(signature) = signature {
                    if signature.as_bytes().is_empty() {
                        bail_validation!("no signature in commit sig")
                    }
                    if signature.as_bytes().len() != SIGNATURE_LENGTH {
                        bail_validation!(
                            "signature ({:?}) length != required ({})",
                            signature.as_bytes(),
                            SIGNATURE_LENGTH
                        )
                    }
                } else {
                    bail_validation!("no signature in commit sig")
                }
            }
        }

        Ok(())
    }
}

/// An extension trait for the [`Commit`] to perform additional actions.
///
/// [`Commit`]: tendermint::block::Commit
pub trait CommitExt {
    /// Get the signed [`Vote`] from the [`Commit`] at the given index.
    ///
    /// [`Commit`]: tendermint::block::Commit
    /// [`Vote`]: tendermint::Vote
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>>;
}

impl CommitExt for Commit {
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>> {
        let sig =
            self.signatures
                .get(signature_idx)
                .cloned()
                .ok_or(Error::InvalidSignatureIndex(
                    signature_idx,
                    self.height.value(),
                ))?;

        let (validator_address, timestamp, signature) = match sig {
            CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp,
                signature,
            }
            | CommitSig::BlockIdFlagNil {
                validator_address,
                timestamp,
                signature,
            } => (validator_address, timestamp, signature),
            CommitSig::BlockIdFlagAbsent => return Err(Error::UnexpectedAbsentSignature),
        };

        let vote = Vote {
            vote_type: vote::Type::Precommit,
            height: self.height,
            round: self.round,
            block_id: Some(self.block_id),
            timestamp: Some(timestamp),
            validator_address,
            validator_index: signature_idx.try_into()?,
            signature,
            extension: Vec::new(),
            extension_signature: None,
        };

        Ok(vote.into_signable_vec(chain_id.clone()))
    }
}

fn is_zero(id: &Id) -> bool {
    matches!(id.hash, Hash::None)
        && matches!(id.part_set_header.hash, Hash::None)
        && id.part_set_header.total == 0
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
    use js_sys::Uint8Array;
    use tendermint::block;
    use tendermint::block::header::Version;
    use tendermint::block::parts;
    use tendermint::block::signed_header::SignedHeader;
    use tendermint::block::Commit;
    use tendermint::block::CommitSig;
    use tendermint::block::Header;
    use tendermint::vote::{Type as VoteType, Vote};
    use tendermint::Signature;

    use wasm_bindgen::prelude::*;

    /// Commit contains the justification (ie. a set of signatures) that a block was
    /// committed by a set of validators.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Commit")]
    pub struct JsCommit {
        /// Block height
        pub height: u64,
        /// Round
        pub round: u32,
        /// Block ID
        pub block_id: JsBlockId,
        /// Signatures
        pub signatures: Vec<JsCommitSig>,
    }

    impl From<Commit> for JsCommit {
        fn from(value: Commit) -> Self {
            JsCommit {
                height: value.height.into(),
                round: value.round.into(),
                block_id: value.block_id.into(),
                signatures: value.signatures.into_iter().map(Into::into).collect(),
            }
        }
    }

    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "CommitSig")]
    pub struct JsCommitSig {
        /// vote type of a validator
        pub vote_type: JsCommitVoteType,
        /// vote, if received
        pub vote: Option<JsCommitVote>,
    }

    impl From<CommitSig> for JsCommitSig {
        fn from(value: CommitSig) -> Self {
            match value {
                CommitSig::BlockIdFlagAbsent => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagAbsent,
                    vote: None,
                },
                CommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagCommit,
                    vote: Some(JsCommitVote {
                        validator_address: validator_address.to_string(),
                        timestamp: timestamp.to_rfc3339(),
                        signature: signature.map(Into::into),
                    }),
                },
                CommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagNil,
                    vote: Some(JsCommitVote {
                        validator_address: validator_address.to_string(),
                        timestamp: timestamp.to_rfc3339(),
                        signature: signature.map(Into::into),
                    }),
                },
            }
        }
    }

    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Signature")]
    pub struct JsSignature(pub Uint8Array);

    impl From<Signature> for JsSignature {
        fn from(value: Signature) -> Self {
            JsSignature(Uint8Array::from(value.as_bytes()))
        }
    }

    impl From<&[u8]> for JsSignature {
        fn from(value: &[u8]) -> Self {
            JsSignature(Uint8Array::from(value))
        }
    }

    #[derive(Clone, Copy, Debug)]
    #[wasm_bindgen(js_name = "CommitVoteType")]
    pub enum JsCommitVoteType {
        /// no vote was received from a validator.
        BlockIdFlagAbsent,
        /// voted for the Commit.BlockID.
        BlockIdFlagCommit,
        /// voted for nil
        BlockIdFlagNil,
    }

    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "CommitVote")]
    pub struct JsCommitVote {
        pub validator_address: String,
        pub timestamp: String,
        pub signature: Option<JsSignature>,
    }

    /// Version contains the protocol version for the blockchain and the application.
    #[derive(Clone, Copy, Debug)]
    #[wasm_bindgen(js_name = "ProtocolVersion")]
    pub struct JsProtocolVersion {
        /// blockchain version
        pub block: u64,
        /// app version
        pub app: u64,
    }

    impl From<Version> for JsProtocolVersion {
        fn from(value: Version) -> Self {
            JsProtocolVersion {
                block: value.block,
                app: value.app,
            }
        }
    }
    /// Block parts header
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "PartsHeader")]
    pub struct JsPartsHeader {
        /// Number of parts in this block
        pub total: u32,
        /// Hash of the parts set header
        pub hash: String,
    }

    impl From<parts::Header> for JsPartsHeader {
        fn from(value: parts::Header) -> Self {
            JsPartsHeader {
                total: value.total,
                hash: value.hash.to_string(),
            }
        }
    }

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

    /// Block Header values contain metadata about the block and about the consensus,
    /// as well as commitments to the data in the current block, the previous block,
    /// and the results returned by the application.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Header")]
    pub struct JsHeader {
        /// Header version
        pub version: JsProtocolVersion,
        /// Chain ID
        pub chain_id: String,
        /// Current block height
        pub height: u64,
        /// Current timestamp encoded as rfc3339
        pub time: String,
        /// Previous block info
        pub last_block_id: Option<JsBlockId>,
        /// Commit from validators from the last block
        pub last_commit_hash: Option<String>,
        /// Merkle root of transaction hashes
        pub data_hash: Option<String>,
        /// Validators for the current block
        pub validators_hash: String,
        /// Validators for the next block
        pub next_validators_hash: String,
        /// Consensus params for the current block
        pub consensus_hash: String,
        /// State after txs from the previous block
        pub app_hash: String,
        /// Root hash of all results from the txs from the previous block
        pub last_results_hash: Option<String>,
        /// Hash of evidence included in the block
        pub evidence_hash: Option<String>,
        /// Original proposer of the block
        pub proposer_address: String,
    }

    impl From<Header> for JsHeader {
        fn from(value: Header) -> Self {
            JsHeader {
                version: value.version.into(),
                chain_id: value.chain_id.to_string(),
                height: value.height.value(),
                time: value.time.to_rfc3339(),
                last_block_id: value.last_block_id.map(Into::into),
                last_commit_hash: value.last_commit_hash.map(|h| h.to_string()),
                data_hash: value.data_hash.map(|h| h.to_string()),
                validators_hash: value.validators_hash.to_string(),
                next_validators_hash: value.next_validators_hash.to_string(),
                consensus_hash: value.consensus_hash.to_string(),
                app_hash: value.app_hash.to_string(),
                last_results_hash: value.last_results_hash.map(|h| h.to_string()),
                evidence_hash: value.evidence_hash.map(|h| h.to_string()),
                proposer_address: value.proposer_address.to_string(),
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    #[wasm_bindgen(js_name = "VoteType")]
    pub enum JsVoteType {
        Prevote,
        Precommit,
    }

    impl From<VoteType> for JsVoteType {
        fn from(value: VoteType) -> Self {
            match value {
                VoteType::Prevote => JsVoteType::Prevote,
                VoteType::Precommit => JsVoteType::Precommit,
            }
        }
    }

    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Vote")]
    pub struct JsVote {
        /// Type of vote (prevote or precommit)
        pub vote_type: JsVoteType,
        /// Block height
        pub height: u64,
        /// Round
        pub round: u32,
        /// Block ID
        pub block_id: Option<JsBlockId>,
        /// Timestamp
        pub timestamp: Option<String>,
        /// Validator address
        pub validator_address: String,
        /// Validator index
        pub validator_index: u32,
        /// Signature
        pub signature: Option<JsSignature>,
        /// Vote extension provided by the application. Only valid for precommit messages.
        pub extension: Vec<u8>,
        /// Vote extension signature by the validator Only valid for precommit messages.
        pub extension_signature: Option<JsSignature>,
    }

    impl From<Vote> for JsVote {
        fn from(value: Vote) -> Self {
            JsVote {
                vote_type: value.vote_type.into(),
                height: value.height.value(),
                round: value.round.into(),
                block_id: value.block_id.map(Into::into),
                timestamp: value.timestamp.map(|ts| ts.to_rfc3339()),
                validator_address: value.validator_address.to_string(),
                validator_index: value.validator_index.into(),
                signature: value.signature.map(Into::into),
                extension: value.extension,
                extension_signature: value.extension_signature.map(Into::into),
            }
        }
    }

    /// Signed block headers
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "SignedHeader")]
    pub struct JsSignedHeader {
        /// Signed block headers
        pub header: JsHeader,
        /// Commit containing signatures for the header
        pub commit: JsCommit,
    }

    impl From<SignedHeader> for JsSignedHeader {
        fn from(value: SignedHeader) -> Self {
            JsSignedHeader {
                header: value.header.into(),
                commit: value.commit.into(),
            }
        }
    }
}

/// uniffi types
#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::block::signed_header::SignedHeader as TendermintSignedHeader;
    use tendermint::block::{
        Commit as TendermintCommit, CommitSig as TendermintCommitSig, Header as TendermintHeader,
        Height,
    };
    use uniffi::{Enum, Record};

    use crate::error::UniffiConversionError;
    use crate::hash::Hash;
    use crate::state::UniffiAccountId;
    use crate::uniffi_types::{AppHash, BlockId, ChainId, ProtocolVersion, Signature, Time};

    /// Signed block headers
    #[derive(Record)]
    pub struct SignedHeader {
        /// Signed block headers
        pub header: Header,
        /// Commit containing signatures for the header
        pub commit: Commit,
    }

    impl TryFrom<TendermintSignedHeader> for SignedHeader {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintSignedHeader) -> Result<Self, Self::Error> {
            Ok(SignedHeader {
                header: value.header.into(),
                commit: value.commit.try_into()?,
            })
        }
    }

    impl TryFrom<SignedHeader> for TendermintSignedHeader {
        type Error = UniffiConversionError;

        fn try_from(value: SignedHeader) -> Result<Self, Self::Error> {
            TendermintSignedHeader::new(value.header.try_into()?, value.commit.try_into()?)
                .map_err(|_| UniffiConversionError::InvalidSignedHeader)
        }
    }

    /// Commit contains the justification (ie. a set of signatures) that a block was
    /// committed by a set of validators.
    #[derive(Record)]
    pub struct Commit {
        /// Block height
        pub height: u64,
        /// Round
        pub round: u32,
        /// Block ID
        pub block_id: BlockId,
        /// Signatures
        pub signatures: Vec<CommitSig>,
    }

    impl TryFrom<TendermintCommit> for Commit {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintCommit) -> Result<Self, Self::Error> {
            Ok(Commit {
                height: value.height.value(),
                round: value.round.value(),
                block_id: value.block_id.into(),
                signatures: value
                    .signatures
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            })
        }
    }

    impl TryFrom<Commit> for TendermintCommit {
        type Error = UniffiConversionError;

        fn try_from(value: Commit) -> Result<Self, Self::Error> {
            Ok(TendermintCommit {
                height: value
                    .height
                    .try_into()
                    .map_err(|_| UniffiConversionError::HeaderHeightOutOfRange)?,
                round: value
                    .round
                    .try_into()
                    .map_err(|_| UniffiConversionError::InvalidRoundIndex)?,
                block_id: value.block_id.try_into()?,
                signatures: value
                    .signatures
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<Result<_, _>>()?,
            })
        }
    }

    uniffi::custom_type!(TendermintCommit, Commit, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.try_into().expect("valid tendermint timestamp")
    });

    /// CommitSig represents a signature of a validator. It’s a part of the Commit and can
    /// be used to reconstruct the vote set given the validator set.
    #[derive(Enum)]
    pub enum CommitSig {
        /// no vote was received from a validator.
        BlockIdFlagAbsent,
        /// voted for the Commit.BlockID.
        BlockIdFlagCommit {
            /// Validator address
            validator_address: UniffiAccountId,
            /// Timestamp
            timestamp: Time,
            /// Signature of vote
            signature: Option<Signature>,
        },
        /// voted for nil
        BlockIdFlagNil {
            /// Validator address
            validator_address: UniffiAccountId,
            /// Timestamp
            timestamp: Time,
            /// Signature of vote
            signature: Option<Signature>,
        },
    }

    impl TryFrom<TendermintCommitSig> for CommitSig {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintCommitSig) -> Result<Self, Self::Error> {
            Ok(match value {
                TendermintCommitSig::BlockIdFlagAbsent => CommitSig::BlockIdFlagAbsent,
                TendermintCommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => CommitSig::BlockIdFlagCommit {
                    validator_address: validator_address.into(),
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(Into::into),
                },
                TendermintCommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => CommitSig::BlockIdFlagNil {
                    validator_address: validator_address.into(),
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(Into::into),
                },
            })
        }
    }

    impl TryFrom<CommitSig> for TendermintCommitSig {
        type Error = UniffiConversionError;

        fn try_from(value: CommitSig) -> Result<Self, Self::Error> {
            Ok(match value {
                CommitSig::BlockIdFlagAbsent => TendermintCommitSig::BlockIdFlagAbsent,
                CommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => TendermintCommitSig::BlockIdFlagCommit {
                    validator_address: validator_address.try_into()?,
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(TryInto::try_into).transpose()?,
                },
                CommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => TendermintCommitSig::BlockIdFlagNil {
                    validator_address: validator_address.try_into()?,
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(TryInto::try_into).transpose()?,
                },
            })
        }
    }

    /// Block Header values contain metadata about the block and about the consensus,
    /// as well as commitments to the data in the current block, the previous block,
    /// and the results returned by the application.
    #[derive(Record)]
    pub struct Header {
        /// Header version
        pub version: ProtocolVersion,
        /// Chain ID
        pub chain_id: ChainId,
        /// Current block height
        pub height: Height,
        /// Current timestamp
        pub time: Time,
        /// Previous block info
        pub last_block_id: Option<BlockId>,
        /// Commit from validators from the last block
        pub last_commit_hash: Option<Hash>,
        /// Merkle root of transaction hashes
        pub data_hash: Option<Hash>,
        /// Validators for the current block
        pub validators_hash: Hash,
        /// Validators for the next block
        pub next_validators_hash: Hash,
        /// Consensus params for the current block
        pub consensus_hash: Hash,
        /// State after txs from the previous block
        pub app_hash: AppHash,
        /// Root hash of all results from the txs from the previous block
        pub last_results_hash: Option<Hash>,
        /// Hash of evidence included in the block
        pub evidence_hash: Option<Hash>,
        /// Original proposer of the block
        pub proposer_address: UniffiAccountId,
    }

    impl TryFrom<Header> for TendermintHeader {
        type Error = UniffiConversionError;

        fn try_from(value: Header) -> std::result::Result<Self, Self::Error> {
            Ok(TendermintHeader {
                version: value.version,
                chain_id: value.chain_id.try_into()?,
                height: value.height,
                time: value.time.try_into()?,
                last_block_id: value.last_block_id.map(TryInto::try_into).transpose()?,
                last_commit_hash: value.last_commit_hash,
                data_hash: value.data_hash,
                validators_hash: value.validators_hash,
                next_validators_hash: value.next_validators_hash,
                consensus_hash: value.consensus_hash,
                app_hash: value.app_hash.try_into()?,
                last_results_hash: value.last_results_hash,
                evidence_hash: value.evidence_hash,
                proposer_address: value.proposer_address.try_into()?,
            })
        }
    }

    impl From<TendermintHeader> for Header {
        fn from(value: TendermintHeader) -> Self {
            Header {
                version: value.version,
                chain_id: value.chain_id.into(),
                height: value.height,
                time: value.time.try_into().expect("valid time in tendermint"),
                last_block_id: value.last_block_id.map(Into::into),
                last_commit_hash: value.last_commit_hash,
                data_hash: value.data_hash,
                validators_hash: value.validators_hash,
                next_validators_hash: value.next_validators_hash,
                consensus_hash: value.consensus_hash,
                app_hash: value.app_hash.into(),
                last_results_hash: value.last_results_hash,
                evidence_hash: value.evidence_hash,
                proposer_address: value.proposer_address.into(),
            }
        }
    }

    uniffi::custom_type!(TendermintHeader, Header, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashExt;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_commit() -> Commit {
        serde_json::from_str(r#"{
          "height": "1",
          "round": 0,
          "block_id": {
            "hash": "17F7D5108753C39714DCA67E6A73CE855C6EA9B0071BBD4FFE5D2EF7F3973BFC",
            "parts": {
              "total": 1,
              "hash": "BEEBB79CDA7D0574B65864D3459FAC7F718B82496BD7FE8B6288BF0A98C8EA22"
            }
          },
          "signatures": [
            {
              "block_id_flag": 2,
              "validator_address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
              "timestamp": "2023-06-23T10:40:48.769228056Z",
              "signature": "HNn4c02eCt2+nGuBs55L8f3DAz9cgy9psLFuzhtg2XCWnlkt2V43TX2b54hQNi7C0fepBEteA3GC01aJM/JJCg=="
            }
          ]
        }"#).unwrap()
    }

    fn sample_header() -> Header {
        serde_json::from_str(r#"{
          "version": {
            "block": "11",
            "app": "1"
          },
          "chain_id": "private",
          "height": "1",
          "time": "2023-06-23T10:40:48.410305119Z",
          "last_block_id": {
            "hash": "",
            "parts": {
              "total": 0,
              "hash": ""
            }
          },
          "last_commit_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "data_hash": "3D96B7D238E7E0456F6AF8E7CDF0A67BD6CF9C2089ECB559C659DCAA1F880353",
          "validators_hash": "64AEB6CA415A37540650FC04471974CE4FE88884CDD3300DF7BB27C1786871E9",
          "next_validators_hash": "64AEB6CA415A37540650FC04471974CE4FE88884CDD3300DF7BB27C1786871E9",
          "consensus_hash": "C0B6A634B72AE9687EA53B6D277A73ABA1386BA3CFC6D0F26963602F7F6FFCD6",
          "app_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "last_results_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "evidence_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "proposer_address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53"
        }"#).unwrap()
    }

    #[test]
    fn block_id_is_zero() {
        let mut block_id = sample_commit().block_id;
        assert!(!is_zero(&block_id));

        block_id.hash = Hash::None;
        assert!(!is_zero(&block_id));

        block_id.part_set_header.hash = Hash::None;
        assert!(!is_zero(&block_id));

        block_id.part_set_header.total = 0;
        assert!(is_zero(&block_id));
    }

    #[test]
    fn header_validate_basic() {
        sample_header().validate_basic().unwrap();
    }

    #[test]
    fn header_validate_invalid_block_version() {
        let mut header = sample_header();
        header.version.block = 1;

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_zero_height() {
        let mut header = sample_header();
        header.height = 0u32.into();

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_missing_last_block_id() {
        let mut header = sample_header();
        header.height = 2u32.into();

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_genesis_with_last_block_id() {
        let mut header = sample_header();

        header.last_block_id = Some(Id {
            hash: Hash::default_sha256(),
            ..Id::default()
        });

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn commit_validate_basic() {
        sample_commit().validate_basic().unwrap();
    }

    #[test]
    fn commit_validate_invalid_block_id() {
        let mut commit = sample_commit();
        commit.block_id.hash = Hash::None;
        commit.block_id.part_set_header.hash = Hash::None;
        commit.block_id.part_set_header.total = 0;

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn commit_validate_no_signatures() {
        let mut commit = sample_commit();
        commit.signatures = vec![];

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn commit_validate_absent() {
        let mut commit = sample_commit();
        commit.signatures[0] = CommitSig::BlockIdFlagAbsent;

        commit.validate_basic().unwrap();
    }

    #[test]
    fn commit_validate_no_signature_in_sig() {
        let mut commit = sample_commit();
        let CommitSig::BlockIdFlagCommit {
            validator_address,
            timestamp,
            ..
        } = commit.signatures[0].clone()
        else {
            unreachable!()
        };
        commit.signatures[0] = CommitSig::BlockIdFlagCommit {
            signature: None,
            timestamp,
            validator_address,
        };

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn vote_sign_bytes() {
        let commit = sample_commit();

        let signable_bytes = commit
            .vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 0)
            .unwrap();

        assert_eq!(
            signable_bytes,
            vec![
                108u8, 8, 2, 17, 1, 0, 0, 0, 0, 0, 0, 0, 34, 72, 10, 32, 23, 247, 213, 16, 135, 83,
                195, 151, 20, 220, 166, 126, 106, 115, 206, 133, 92, 110, 169, 176, 7, 27, 189, 79,
                254, 93, 46, 247, 243, 151, 59, 252, 18, 36, 8, 1, 18, 32, 190, 235, 183, 156, 218,
                125, 5, 116, 182, 88, 100, 211, 69, 159, 172, 127, 113, 139, 130, 73, 107, 215,
                254, 139, 98, 136, 191, 10, 152, 200, 234, 34, 42, 12, 8, 176, 237, 213, 164, 6,
                16, 152, 250, 229, 238, 2, 50, 7, 112, 114, 105, 118, 97, 116, 101
            ]
        );
    }

    #[test]
    fn vote_sign_bytes_absent_signature() {
        let mut commit = sample_commit();
        commit.signatures[0] = CommitSig::BlockIdFlagAbsent;

        let res = commit.vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 0);

        assert!(matches!(res, Err(Error::UnexpectedAbsentSignature)));
    }

    #[test]
    fn vote_sign_bytes_non_existent_signature() {
        let commit = sample_commit();

        let res = commit.vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 3);

        assert!(matches!(res, Err(Error::InvalidSignatureIndex(..))));
    }
}
