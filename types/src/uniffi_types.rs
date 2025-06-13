//! Misc types used by uniffi

use tendermint::block::parts::Header as TendermintPartsHeader;
use tendermint::block::Id as TendermintBlockId;
use tendermint::chain::Id as TendermintChainId;
use tendermint::hash::{AppHash as TendermintAppHash, Hash};
use tendermint::signature::Signature as TendermintSignature;
use tendermint::time::Time as TendermintTime;
use tendermint::vote::Type as VoteType;
use tendermint::vote::Vote as TendermintVote;
use tendermint_proto::google::protobuf::Any as ProtobufAny;
use uniffi::Record;

use crate::error::UniffiConversionError;
use crate::state::UniffiAccountId;

/// Version contains the protocol version for the blockchain and the application.
pub type ProtocolVersion = tendermint::block::header::Version;

/// Version contains the protocol version for the blockchain and the application.
#[uniffi::remote(Record)]
pub struct ProtocolVersion {
    /// blockchain version
    pub block: u64,
    /// app version
    pub app: u64,
}

/// AppHash is usually a SHA256 hash, but in reality it can be any kind of data
#[derive(Record)]
pub struct AppHash {
    /// AppHash value
    pub hash: Vec<u8>,
}

impl TryFrom<AppHash> for TendermintAppHash {
    type Error = UniffiConversionError;

    fn try_from(value: AppHash) -> Result<Self, Self::Error> {
        Ok(TendermintAppHash::try_from(value.hash).expect("conversion to be infallible"))
    }
}

impl From<TendermintAppHash> for AppHash {
    fn from(value: TendermintAppHash) -> Self {
        AppHash {
            hash: value.as_bytes().to_vec(),
        }
    }
}

uniffi::custom_type!(TendermintAppHash, AppHash, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.into(),
});

/// Chain identifier (e.g. ‘gaia-9000’)
#[derive(Record)]
pub struct ChainId {
    id: String,
}

impl From<TendermintChainId> for ChainId {
    fn from(value: TendermintChainId) -> Self {
        ChainId {
            id: value.to_string(),
        }
    }
}

impl TryFrom<ChainId> for TendermintChainId {
    type Error = UniffiConversionError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        TendermintChainId::try_from(value.id)
            .map_err(|_| UniffiConversionError::InvalidChainIdLength)
    }
}

uniffi::custom_type!(TendermintChainId, ChainId, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.into()
});

/// Block parts header
#[derive(Record)]
pub struct PartsHeader {
    /// Number of parts in this block
    pub total: u32,
    /// Hash of the parts set header
    pub hash: Hash,
}

impl TryFrom<PartsHeader> for TendermintPartsHeader {
    type Error = UniffiConversionError;

    fn try_from(value: PartsHeader) -> Result<Self, Self::Error> {
        TendermintPartsHeader::new(value.total, value.hash)
            .map_err(|e| UniffiConversionError::InvalidPartsHeader { msg: e.to_string() })
    }
}

impl From<TendermintPartsHeader> for PartsHeader {
    fn from(value: TendermintPartsHeader) -> Self {
        PartsHeader {
            total: value.total,
            hash: value.hash,
        }
    }
}

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

/// Tendermint timestamp
#[derive(Record)]
pub struct Time {
    ts: i64,
    nanos: u32,
}

impl TryFrom<TendermintTime> for Time {
    type Error = UniffiConversionError;

    fn try_from(value: TendermintTime) -> Result<Self, Self::Error> {
        const NANOSECONDS_IN_SECOND: i128 = 1_000_000_000;
        let ts = value.unix_timestamp_nanos();
        let nanos: u32 = (ts % NANOSECONDS_IN_SECOND)
            .try_into()
            .expect("remainder to fit");
        let ts: i64 = (ts / NANOSECONDS_IN_SECOND)
            .try_into()
            .map_err(|_| UniffiConversionError::TimestampOutOfRange)?;
        Ok(Time { ts, nanos })
    }
}

impl TryFrom<Time> for TendermintTime {
    type Error = UniffiConversionError;

    fn try_from(value: Time) -> Result<Self, Self::Error> {
        let Time { ts, nanos } = value;
        TendermintTime::from_unix_timestamp(ts, nanos)
            .map_err(|_| UniffiConversionError::TimestampOutOfRange)
    }
}

uniffi::custom_type!(TendermintTime, Time, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.try_into().expect("valid data")
});

/// Signature
#[derive(Record)]
pub struct Signature {
    signature: Vec<u8>,
}

impl TryFrom<Signature> for TendermintSignature {
    type Error = UniffiConversionError;

    fn try_from(value: Signature) -> Result<Self, Self::Error> {
        TendermintSignature::new(&value.signature)
            .map_err(|_| UniffiConversionError::InvalidSignatureLength)?
            .ok_or(UniffiConversionError::InvalidSignatureLength)
    }
}

impl From<TendermintSignature> for Signature {
    fn from(value: TendermintSignature) -> Self {
        Signature {
            signature: value.into_bytes(),
        }
    }
}

uniffi::custom_type!(TendermintSignature, Signature, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.into()
});

/// Types of votes
#[uniffi::remote(Enum)]
#[repr(u8)]
pub enum VoteType {
    Prevote = 1,
    Precommit = 2,
}

/// Votes are signed messages from validators for a particular block which include information
/// about the validator signing it.
#[derive(Record)]
pub struct Vote {
    /// Type of vote (prevote or precommit)
    pub vote_type: VoteType,
    /// Block height
    pub height: BlockHeight,
    /// Round
    pub round: u32,
    /// Block ID
    pub block_id: Option<BlockId>,
    /// Timestamp
    pub timestamp: Option<Time>,
    /// Validator address
    pub validator_address: UniffiAccountId,
    /// Validator index
    pub validator_index: u32,
    /// Signature
    pub signature: Option<Signature>,
    /// Vote extension provided by the application. Only valid for precommit messages.
    pub extension: Vec<u8>,
    /// Vote extension signature by the validator Only valid for precommit messages.
    pub extension_signature: Option<Signature>,
}

impl TryFrom<Vote> for TendermintVote {
    type Error = UniffiConversionError;

    fn try_from(value: Vote) -> Result<Self, Self::Error> {
        Ok(TendermintVote {
            vote_type: value.vote_type,
            height: value.height.try_into()?,
            round: value
                .round
                .try_into()
                .map_err(|_| UniffiConversionError::InvalidRoundIndex)?,
            block_id: value.block_id.map(TryInto::try_into).transpose()?,
            timestamp: value.timestamp.map(TryInto::try_into).transpose()?,
            validator_address: value.validator_address.try_into()?,
            validator_index: value
                .validator_index
                .try_into()
                .map_err(|_| UniffiConversionError::InvalidValidatorIndex)?,
            signature: value.signature.map(TryInto::try_into).transpose()?,
            extension: value.extension,
            extension_signature: value
                .extension_signature
                .map(TryInto::try_into)
                .transpose()?,
        })
    }
}

impl TryFrom<TendermintVote> for Vote {
    type Error = UniffiConversionError;

    fn try_from(value: TendermintVote) -> Result<Self, Self::Error> {
        Ok(Vote {
            vote_type: value.vote_type,
            height: value.height.into(),
            round: value.round.value(),
            block_id: value.block_id.map(Into::into),
            timestamp: value.timestamp.map(TryInto::try_into).transpose()?,
            validator_address: value.validator_address.into(),
            validator_index: value.validator_index.value(),
            signature: value.signature.map(Into::into),
            extension: value.extension,
            extension_signature: value.extension_signature.map(Into::into),
        })
    }
}

uniffi::custom_type!(TendermintVote, Vote, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.try_into().expect("valid tendermint time")
});

/// Any contains an arbitrary serialized protocol buffer message along with a URL that
/// describes the type of the serialized message.
#[uniffi::remote(Record)]
pub struct ProtobufAny {
    /// A URL/resource name that uniquely identifies the type of the serialized protocol
    /// buffer message. This string must contain at least one “/” character. The last
    /// segment of the URL’s path must represent the fully qualified name of the type
    /// (as in path/google.protobuf.Duration). The name should be in a canonical form
    /// (e.g., leading “.” is not accepted).
    pub type_url: String,
    /// Must be a valid serialized protocol buffer of the above specified type.
    pub value: Vec<u8>,
}

use tendermint::block::Height as TendermintHeight;

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

use bytes::Bytes as RawBytes;

/// Array of bytes
#[derive(Record)]
pub struct Bytes {
    /// Stored bytes
    bytes: Vec<u8>,
}

impl From<Bytes> for RawBytes {
    fn from(value: Bytes) -> Self {
        value.bytes.into()
    }
}

impl From<RawBytes> for Bytes {
    fn from(value: RawBytes) -> Self {
        Bytes {
            bytes: value.into(),
        }
    }
}

uniffi::custom_type!(RawBytes, Bytes, {
    remote,
    try_lift: |value| Ok(value.into()),
    lower: |value| value.into()
});
