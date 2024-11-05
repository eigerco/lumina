use std::convert::Infallible;

use cosmrs::ErrorReport;
use tonic::Status;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    TonicError(#[from] Status),

    #[error(transparent)]
    TendermintError(#[from] celestia_tendermint::Error),

    #[error(transparent)]
    CosmrsError(#[from] ErrorReport),

    #[error(transparent)]
    TendermintProtoError(#[from] celestia_tendermint_proto::Error),

    #[error("Failed to parse response")]
    FailedToParseResponse,

    #[error("Unexpected response type")]
    UnexpectedResponseType(String),

    /// Unreachable. Added to appease try_into conversion for GrpcClient method macro
    #[error(transparent)]
    Infallible(#[from] Infallible),
}
