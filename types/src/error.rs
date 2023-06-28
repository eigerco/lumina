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
    Validation(#[from] crate::ValidationError),

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

    #[error("Invalid share size: {0}")]
    InvalidShareSize(usize),

    #[error("Invalid namespaced hash")]
    InvalidNamespacedHash,

    #[error("Invalid namespace v0")]
    InvalidNamespaceV0,

    #[error("Invalid index of signature in commit {0}, height {1}")]
    InvalidSignatureIndex(usize, u64),

    #[error("Unexpected absent commit signature")]
    UnexpectedAbsentSignature,

    #[error("Not enough voting power to verify commit: has {0}, required: {1}")]
    NotEnoughVotingPower(u64, u64),

    #[error("Verification error: {0}")]
    Verification(#[from] VerificationError),
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Not enought voiting power (got {0}, needed {1})")]
    NotEnoughVotingPower(u64, u64),

    #[error("{0}")]
    Other(String),
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
pub(crate) use bail_verification;
pub(crate) use verification_error;
