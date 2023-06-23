pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unsupported namesapce version: {0}")]
    UnsupportedNamespaceVersion(u8),

    #[error("Invalid namespace ID size: {0}")]
    InvalidNamespaceIdSize(usize),

    #[error(transparent)]
    Tendermint(#[from] tendermint::Error),

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
}
