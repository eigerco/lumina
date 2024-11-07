use crate::consts::appconsts;

/// Alias for a `Result` with the error type [`celestia_types::Error`].
///
/// [`celestia_types::Error`]: crate::Error
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with [`celestia_types`].
///
/// [`celestia_types`]: crate
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unsupported namespace version.
    #[error("Unsupported namespace version: {0}")]
    UnsupportedNamespaceVersion(u8),

    /// Invalid namespace size.
    #[error("Invalid namespace size")]
    InvalidNamespaceSize,

    /// Error propagated from the [`celestia_tendermint`].
    #[error(transparent)]
    Tendermint(#[from] celestia_tendermint::Error),

    /// Error propagated from the [`celestia_tendermint_proto`].
    #[error(transparent)]
    Protobuf(#[from] celestia_tendermint_proto::Error),

    /// Error propagated from the [`cid::multihash`].
    #[error(transparent)]
    Multihash(#[from] cid::multihash::Error),

    /// Error returned when trying to compute new or parse existing CID. See [`blockstore::block`]
    #[error(transparent)]
    CidError(#[from] blockstore::block::CidError),

    /// Error propagated from the [`leopard_codec`].
    #[error(transparent)]
    LeopardCodec(#[from] leopard_codec::LeopardError),

    /// Missing header.
    #[error("Missing header")]
    MissingHeader,

    /// Missing commit.
    #[error("Missing commit")]
    MissingCommit,

    /// Missing validator set.
    #[error("Missing validator set")]
    MissingValidatorSet,

    /// Missing data availability header.
    #[error("Missing data availability header")]
    MissingDataAvailabilityHeader,

    /// Missing proof.
    #[error("Missing proof")]
    MissingProof,

    /// Missing shares.
    #[error("Missing shares")]
    MissingShares,

    /// Wrong proof type.
    #[error("Wrong proof type")]
    WrongProofType,

    /// Unsupported share version.
    #[error("Unsupported share version: {0}")]
    UnsupportedShareVersion(u8),

    /// Invalid share size.
    #[error("Invalid share size: {0}")]
    InvalidShareSize(usize),

    /// Invalid nmt leaf size.
    #[error("Invalid nmt leaf size: {0}")]
    InvalidNmtLeafSize(usize),

    /// Invalid nmt node order.
    #[error("Invalid nmt node order")]
    InvalidNmtNodeOrder,

    /// Share sequence length exceeded.
    #[error(
        "Sequence len must fit into {} bytes, got value {0}",
        appconsts::SEQUENCE_LEN_BYTES
    )]
    ShareSequenceLenExceeded(usize),

    /// Invalid namespace in version 0.
    #[error("Invalid namespace v0")]
    InvalidNamespaceV0,

    /// Invalid namespace in version 255.
    #[error("Invalid namespace v255")]
    InvalidNamespaceV255,

    /// Invalid namespaced hash.
    #[error(transparent)]
    InvalidNamespacedHash(#[from] nmt_rs::InvalidNamespacedHash),

    /// Invalid index of signature in commit.
    #[error("Invalid index of signature in commit {0}, height {1}")]
    InvalidSignatureIndex(usize, u64),

    /// Invalid axis.
    #[error("Invalid axis type: {0}")]
    InvalidAxis(i32),

    /// Invalid Shwap proof type in Protobuf.
    #[error("Invalid proof type: {0}")]
    InvalidShwapProofType(i32),

    /// Range proof verification error.
    #[error("Range proof verification failed: {0:?}")]
    RangeProofError(nmt_rs::simple_merkle::error::RangeProofError),

    /// Row root computed from shares doesn't match one received in `DataAvailabilityHeaderz
    #[error("Computed root doesn't match received one")]
    RootMismatch,

    /// Unexpected signature in absent commit.
    #[error("Unexpected absent commit signature")]
    UnexpectedAbsentSignature,

    /// Error that happened during validation.
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Error that happened during verification.
    #[error("Verification error: {0}")]
    Verification(#[from] VerificationError),

    /// Max share version exceeded.
    #[error(
        "Share version has to be at most {}, got {0}",
        appconsts::MAX_SHARE_VERSION
    )]
    MaxShareVersionExceeded(u8),

    /// An error related to the namespaced merkle tree.
    #[error("Nmt error: {0}")]
    Nmt(&'static str),

    /// Invalid address bech32 prefix.
    #[error("Invalid address prefix: {0}")]
    InvalidAddressPrefix(String),

    /// Invalid size of the address.
    #[error("Invalid address size: {0}")]
    InvalidAddressSize(usize),

    /// Invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Invalid balance denomination.
    #[error("Invalid balance denomination: {0}")]
    InvalidBalanceDenomination(String),

    /// Invalid balance amount.
    #[error("Invalid balance amount: {0}")]
    InvalidBalanceAmount(String),

    /// Invalid Public Key
    #[error("Invalid Public Key")]
    InvalidPublicKeyType(String),

    /// Unsupported fraud proof type.
    #[error("Unsupported fraud proof type: {0}")]
    UnsupportedFraudProofType(String),

    /// Data square index out of range.
    #[error("Index ({0}) out of range ({1})")]
    IndexOutOfRange(usize, usize),

    /// Data square index out of range.
    #[error("Data square index out of range. row: {0}, column: {1}")]
    EdsIndexOutOfRange(u16, u16),

    /// Could not create EDS, provided number of shares doesn't form a pefect square.
    #[error("Invalid dimensions of EDS")]
    EdsInvalidDimentions,

    /// Zero block height.
    #[error("Invalid zero block height")]
    ZeroBlockHeight,

    /// Expected first share of a blob
    #[error("Expected first share of a blob")]
    ExpectedShareWithSequenceStart,

    /// Unexpected share from reserved namespace.
    #[error("Unexpected share from reserved namespace")]
    UnexpectedReservedNamespace,

    /// Unexpected start of a new blob
    #[error("Unexpected start of a new blob")]
    UnexpectedSequenceStart,

    /// Metadata mismatch between shares in blob.
    #[error("Metadata mismatch between shares in blob: {0}")]
    BlobSharesMetadataMismatch(String),
}

impl From<prost::DecodeError> for Error {
    fn from(value: prost::DecodeError) -> Self {
        Error::Protobuf(celestia_tendermint_proto::Error::decode_message(value))
    }
}

/// Representation of the errors that can occur when validating data.
///
/// See [`ValidateBasic`]
///
/// [`ValidateBasic`]: crate::ValidateBasic
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Not enough voting power for a commit.
    #[error("Not enought voting power (got {0}, needed {1})")]
    NotEnoughVotingPower(u64, u64),

    /// Other errors that can happen during validation.
    #[error("{0}")]
    Other(String),
}

/// Representation of the errors that can occur when verifying data.
///
/// See [`ExtendedHeader::verify`].
///
/// [`ExtendedHeader::verify`]: crate::ExtendedHeader::verify
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    /// Not enough voting power for a commit.
    #[error("Not enought voting power (got {0}, needed {1})")]
    NotEnoughVotingPower(u64, u64),

    /// Other errors that can happen during verification.
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
