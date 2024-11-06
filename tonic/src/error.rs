use std::convert::Infallible;

use cosmrs::ErrorReport;
use tonic::Status;

/// Alias for a `Result` with the error type [`celestia_tonic::Error`].
///
/// [`celestia_tonic::Error`]: crate::Error
pub type Result<T> = std::result::Result<T, Error>;

/// Representation of all the errors that can occur when interacting with [`celestia_tonic`].
///
/// [`celestia_tonic`]: crate
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Tonic error
    #[error(transparent)]
    TonicError(#[from] Status),

    /// Tendermint Error
    #[error(transparent)]
    TendermintError(#[from] celestia_tendermint::Error),

    /// Cosmrs Error
    #[error(transparent)]
    CosmrsError(#[from] ErrorReport),

    /// Tendermint Proto Error
    #[error(transparent)]
    TendermintProtoError(#[from] celestia_tendermint_proto::Error),

    /// Failed to parse gRPC response
    #[error("Failed to parse response")]
    FailedToParseResponse,

    /// Unexpected reponse type
    #[error("Unexpected response type")]
    UnexpectedResponseType(String),

    /// Unreachable. Added to appease try_into conversion for GrpcClient method macro
    #[error(transparent)]
    Infallible(#[from] Infallible),
}
