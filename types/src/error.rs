use core::fmt::Display;

use crate::types::String;

/// Alias for a `Result` with the error type [`celestia_types::Error`].
///
/// [`celestia_types::Error`]: crate::Error
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with [`celestia_types`].
///
/// [`celestia_types`]: crate
#[derive(Debug)]
pub enum Error {
    /// Unsupported namespace version.
    UnsupportedNamespaceVersion(u8),

    /// Invalid namespace size.
    InvalidNamespaceSize,

    /// Error propagated from the [`celestia_tendermint`].
    Tendermint(celestia_tendermint::Error),

    /// Error propagated from the [`celestia_tendermint_proto`].
    Protobuf(celestia_tendermint_proto::Error),

    /// Error propagated from the [`cid::multihash`].
    Multihash(cid::multihash::Error),

    /// Error returned when trying to compute new or parse existing CID. See [`blockstore::block`]
    CidError(blockstore::block::CidError),

    /// Missing header.
    MissingHeader,

    /// Missing commit.
    MissingCommit,

    /// Missing validator set.
    MissingValidatorSet,

    /// Missing data availability header.
    MissingDataAvailabilityHeader,

    /// Missing proof.
    MissingProof,

    /// Wrong proof type.
    WrongProofType,

    /// Unsupported share version.
    UnsupportedShareVersion(u8),

    /// Invalid share size.
    InvalidShareSize(usize),

    /// Invalid nmt leaf size.
    InvalidNmtLeafSize(usize),

    /// Invalid nmt node order.
    InvalidNmtNodeOrder,

    /// Share sequence length exceeded.
    ShareSequenceLenExceeded(usize),

    /// Invalid namespace in version 0.
    InvalidNamespaceV0,

    /// Invalid namespace in version 255.
    InvalidNamespaceV255,

    /// Invalid namespaced hash.
    InvalidNamespacedHash(nmt_rs::InvalidNamespacedHash),

    /// Invalid index of signature in commit.
    InvalidSignatureIndex(usize, u64),

    /// Invalid axis.
    InvalidAxis(i32),

    /// Range proof verification error.
    RangeProofError(nmt_rs::simple_merkle::error::RangeProofError),

    /// Row root computed from shares doesn't match one received in `DataAvailabilityHeaderz
    RootMismatch,

    /// Unexpected signature in absent commit.
    UnexpectedAbsentSignature,

    /// Error that happened during validation.
    Validation(ValidationError),

    /// Error that happened during verification.
    Verification(VerificationError),

    /// Max share version exceeded.
    MaxShareVersionExceeded(u8),

    /// An error related to the namespaced merkle tree.
    Nmt(&'static str),

    /// Invalid address bech32 prefix.
    InvalidAddressPrefix(String),

    /// Invalid size of the address.
    InvalidAddressSize(usize),

    /// Invalid address.
    InvalidAddress(String),

    /// Invalid balance denomination.
    InvalidBalanceDenomination(String),

    /// Invalid balance amount.
    InvalidBalanceAmount(String),

    /// Unsupported fraud proof type.
    UnsupportedFraudProofType(String),

    /// Data square index out of range.
    EdsIndexOutOfRange(usize),

    /// Could not create EDS, provided number of shares doesn't form a pefect square
    EdsInvalidDimentions,

    /// Zero block height.
    ZeroBlockHeight,
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::UnsupportedNamespaceVersion(version) => {
                write!(f, "Unsupported namespace version: {}", version)
            }
            Error::InvalidNamespaceSize => write!(f, "Invalid namespace size"),
            Error::MissingHeader => write!(f, "Missing header"),
            Error::MissingCommit => write!(f, "Missing commit"),
            Error::MissingValidatorSet => write!(f, "Missing validator set"),
            Error::MissingDataAvailabilityHeader => write!(f, "Missing data availability header"),
            Error::MissingProof => write!(f, "Missing proof"),
            Error::WrongProofType => write!(f, "Wrong proof type"),
            Error::UnsupportedShareVersion(version) => {
                write!(f, "Unsupported share version: {}", version)
            }
            Error::InvalidShareSize(size) => write!(f, "Invalid share size: {}", size),
            Error::InvalidNmtLeafSize(size) => write!(f, "Invalid nmt leaf size: {}", size),
            Error::InvalidNmtNodeOrder => write!(f, "Invalid nmt node order"),
            Error::ShareSequenceLenExceeded(len) => {
                write!(f, "Share sequence length exceeded: {}", len)
            }
            Error::InvalidNamespaceV0 => write!(f, "Invalid namespace in version 0"),
            Error::InvalidNamespaceV255 => write!(f, "Invalid namespace in version 255"),
            Error::InvalidNamespacedHash(e) => write!(f, "Invalid namespaced hash: {}", e),
            Error::InvalidSignatureIndex(index, len) => {
                write!(f, "Invalid signature index: {} (len: {})", index, len)
            }
            Error::InvalidAxis(axis) => write!(f, "Invalid axis: {}", axis),
            Error::RangeProofError(e) => write!(f, "Range proof error: {:?}", e),
            Error::RootMismatch => write!(f, "Root mismatch"),
            Error::UnexpectedAbsentSignature => write!(f, "Unexpected absent signature"),
            Error::MaxShareVersionExceeded(version) => {
                write!(f, "Max share version exceeded: {}", version)
            }
            Error::Nmt(e) => write!(f, "Nmt error: {}", e),
            Error::Multihash(e) => write!(f, "Multihash error: {}", e),
            Error::CidError(e) => write!(f, "CidError: {}", e),
            Error::InvalidAddressPrefix(prefix) => {
                write!(f, "Invalid address bech32 prefix: {}", prefix)
            }
            Error::InvalidAddressSize(size) => write!(f, "Invalid address size: {}", size),
            Error::InvalidAddress(address) => write!(f, "Invalid address: {}", address),
            Error::InvalidBalanceDenomination(denomination) => {
                write!(f, "Invalid balance denomination: {}", denomination)
            }
            Error::InvalidBalanceAmount(amount) => {
                write!(f, "Invalid balance amount: {}", amount)
            }
            Error::UnsupportedFraudProofType(ty) => {
                write!(f, "Unsupported fraud proof type: {}", ty)
            }
            Error::EdsIndexOutOfRange(index) => write!(f, "EDS index out of range: {}", index),
            Error::EdsInvalidDimentions => write!(f, "EDS invalid dimentions"),
            Error::ZeroBlockHeight => write!(f, "Zero block height"),
            Error::Tendermint(error) => write!(f, "Tendermint error: {error}"),
            Error::Protobuf(error) => write!(f, "Protobuf error: {error}"),
            Error::Validation(error) => write!(f, "Validation error: {error}"),
            Error::Verification(error) => write!(f, "Verification error: {error}"),
        }
    }
}

/// Representation of the errors that can occur when validating data.
///
/// See [`ValidateBasic`]
///
/// [`ValidateBasic`]: crate::ValidateBasic
#[derive(Debug)]
pub enum ValidationError {
    /// Not enough voting power for a commit.
    NotEnoughVotingPower(u64, u64),

    /// Other errors that can happen during validation.
    Other(String),
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ValidationError::NotEnoughVotingPower(voting_power, threshold) => write!(
                f,
                "Not enough voting power: {} < {}",
                voting_power, threshold
            ),
            ValidationError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<ValidationError> for Error {
    fn from(error: ValidationError) -> Self {
        Error::Validation(error)
    }
}

/// Representation of the errors that can occur when verifying data.
///
/// See [`ExtendedHeader::verify`].
///
/// [`ExtendedHeader::verify`]: crate::ExtendedHeader::verify
#[derive(Debug)]
pub enum VerificationError {
    /// Not enough voting power for a commit.
    NotEnoughVotingPower(u64, u64),

    /// Other errors that can happen during verification.
    Other(String),
}

impl Display for VerificationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VerificationError::NotEnoughVotingPower(voting_power, threshold) => write!(
                f,
                "Not enough voting power: {} < {}",
                voting_power, threshold
            ),
            VerificationError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<VerificationError> for Error {
    fn from(error: VerificationError) -> Self {
        Error::Verification(error)
    }
}

impl From<celestia_tendermint::Error> for Error {
    fn from(error: celestia_tendermint::Error) -> Self {
        Error::Tendermint(error)
    }
}

impl From<celestia_tendermint_proto::Error> for Error {
    fn from(error: celestia_tendermint_proto::Error) -> Self {
        Error::Protobuf(error)
    }
}

macro_rules! validation_error {
    ($fmt:literal $(,)?) => {
        $crate::ValidationError::Other(format!($fmt))
    };
    ($fmt:literal, $($arg:tt)*) => {
        $crate::ValidationError::Other(format!($fmt, $($arg)*))
    };
}

macro_rules! bail_validation {
    ($($arg:tt)*) => {
        return Err($crate::validation_error!($($arg)*).into())
    };
}

macro_rules! verification_error {
    ($fmt:literal $(,)?) => {
        $crate::VerificationError::Other(format!($fmt))
    };
    ($fmt:literal, $($arg:tt)*) => {
        $crate::VerificationError::Other(format!($fmt, $($arg)*))
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
