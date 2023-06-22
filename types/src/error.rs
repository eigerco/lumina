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

    #[error("Unexpected absent commit signature")]
    UnexpectedAbsentSignature,

    #[error("Not enough voting power to verify commit: has {0}, required: {1}")]
    NotEnoughVotingPower(u64, u64),
}
