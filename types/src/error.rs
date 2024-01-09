use crate::consts::appconsts;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unsupported namesapce version: {0}")]
    UnsupportedNamespaceVersion(u8),

    #[error("Invalid namespace size")]
    InvalidNamespaceSize,

    #[error(transparent)]
    Tendermint(#[from] tendermint::Error),

    #[error(transparent)]
    Protobuf(#[from] tendermint_proto::Error),

    #[error(transparent)]
    Multihash(#[from] cid::multihash::Error),

    #[error(transparent)]
    CidError(#[from] blockstore::block::CidError),

    #[error("Missing header")]
    MissingHeader,

    #[error("Missing commit")]
    MissingCommit,

    #[error("Missing validator set")]
    MissingValidatorSet,

    #[error("Missing data availability header")]
    MissingDataAvailabilityHeader,

    #[error("Missing proof")]
    MissingProof,

    #[error("Wrong proof type")]
    WrongProofType,

    #[error("Unsupported share version: {0}")]
    UnsupportedShareVersion(u8),

    #[error("Invalid share size: {0}")]
    InvalidShareSize(usize),

    #[error("Invalid nmt leaf size: {0}")]
    InvalidNmtLeafSize(usize),

    #[error("Invalid nmt node order")]
    InvalidNmtNodeOrder,

    #[error(
        "Sequence len must fit into {} bytes, got value {0}",
        appconsts::SEQUENCE_LEN_BYTES
    )]
    ShareSequenceLenExceeded(usize),

    #[error("Invalid namespace v0")]
    InvalidNamespaceV0,

    #[error("Invalid namespace v255")]
    InvalidNamespaceV255,

    #[error(transparent)]
    InvalidNamespacedHash(#[from] nmt_rs::InvalidNamespacedHash),

    #[error("Invalid index of signature in commit {0}, height {1}")]
    InvalidSignatureIndex(usize, u64),

    #[error("Invalid axis type: {0}")]
    InvalidAxis(i32),

    #[error("Range proof verification failed: {0:?}")]
    RangeProofError(nmt_rs::simple_merkle::error::RangeProofError),

    #[error("Computed root doesn't match received one")]
    RootMismatch,

    #[error("Unexpected absent commit signature")]
    UnexpectedAbsentSignature,

    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    #[error("Verification error: {0}")]
    Verification(#[from] VerificationError),

    #[error(
        "Share version has to be at most {}, got {0}",
        appconsts::MAX_SHARE_VERSION
    )]
    MaxShareVersionExceeded(u8),

    #[error("Nmt error: {0}")]
    Nmt(&'static str),

    #[error("Invalid address prefix: {0}")]
    InvalidAddressPrefix(String),

    #[error("Invalid address size: {0}")]
    InvalidAddressSize(usize),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Invalid balance denomination: {0}")]
    InvalidBalanceDenomination(String),

    #[error("Invalid balance amount: {0}")]
    InvalidBalanceAmount(String),

    #[error("Unsupported fraud proof type: {0}")]
    UnsupportedFraudProofType(String),

    #[error("Data square index out of range: {0}")]
    EdsIndexOutOfRange(usize),

    #[error("Invalid dimensions of EDS")]
    EdsInvalidDimentions,

    #[error("Invalid zero block height")]
    ZeroBlockHeight,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Not enought voiting power (got {0}, needed {1})")]
    NotEnoughVotingPower(u64, u64),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Not enought voiting power (got {0}, needed {1})")]
    NotEnoughVotingPower(u64, u64),

    #[error("{0}")]
    Other(String),
}

macro_rules! validation_error {
    ($fmt:literal $(,)?) => {
        $crate::ValidationError::Other(std::format!($fmt))
    };
    ($fmt:literal, $($arg:tt)*) => {
        $crate::ValidationError::Other(std::format!($fmt, $($arg)*))
    };
}

macro_rules! bail_validation {
    ($($arg:tt)*) => {
        return Err($crate::validation_error!($($arg)*).into())
    };
}

macro_rules! verification_error {
    ($fmt:literal $(,)?) => {
        $crate::VerificationError::Other(std::format!($fmt))
    };
    ($fmt:literal, $($arg:tt)*) => {
        $crate::VerificationError::Other(std::format!($fmt, $($arg)*))
    };
}

macro_rules! bail_verification {
    ($($arg:tt)*) => {
        return Err($crate::verification_error!($($arg)*).into())
    };
}

// NOTE: This need to be always after the macro definitions
pub(crate) use bail_validation;
pub(crate) use bail_verification;
pub(crate) use validation_error;
pub(crate) use verification_error;
