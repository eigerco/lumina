use celestia_types::hash::Hash;
use celestia_types::state::ErrorCode;
use k256::ecdsa::signature::Error as SignatureError;
use tonic::Status;

use crate::abci_proofs::ProofError;

/// Alias for a `Result` with the error type [`celestia_grpc::Error`].
///
/// [`celestia_grpc::Error`]: crate::Error
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with [`GrpcClient`].
///
/// [`GrpcClient`]: crate::GrpcClient
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Tonic error
    #[error(transparent)]
    TonicError(Box<Status>),

    /// Transport error
    #[cfg(not(target_arch = "wasm32"))]
    #[error("Transport: {0}")]
    TransportError(#[from] tonic::transport::Error),

    /// Tendermint Error
    #[error(transparent)]
    TendermintError(#[from] tendermint::Error),

    /// Celestia types error
    #[error(transparent)]
    CelestiaTypesError(#[from] celestia_types::Error),

    /// Tendermint Proto Error
    #[error(transparent)]
    TendermintProtoError(#[from] tendermint_proto::Error),

    /// Failed to parse gRPC response
    #[error("Failed to parse response")]
    FailedToParseResponse,

    /// Unexpected reponse type
    #[error("Unexpected response type")]
    UnexpectedResponseType(String),

    /// Empty blob submission list
    #[error("Attempted to submit blob transaction with empty blob list")]
    TxEmptyBlobList,

    /// Broadcasting transaction failed
    #[error("Broadcasting transaction {0} failed; code: {1}, error: {2}")]
    TxBroadcastFailed(Hash, ErrorCode, String),

    /// Executing transaction failed
    #[error("Transaction {0} execution failed; code: {1}, error: {2}")]
    TxExecutionFailed(Hash, ErrorCode, String),

    /// Transaction was evicted from the mempool
    #[error("Transaction {0} was evicted from the mempool")]
    TxEvicted(Hash),

    /// Transaction wasn't found, it was likely rejected
    #[error("Transaction {0} wasn't found, it was likely rejected")]
    TxNotFound(Hash),

    /// Provided public key differs from one associated with account
    #[error("Provided public key differs from one associated with account")]
    PublicKeyMismatch,

    /// ABCI proof verification has failed
    #[error("ABCI proof verification has failed: {0}")]
    AbciProof(#[from] ProofError),

    /// ABCI query returned an error
    #[error("ABCI query returned an error (code {0}): {1}")]
    AbciQuery(ErrorCode, String),

    /// Signing error
    #[error(transparent)]
    SigningError(#[from] SignatureError),

    /// No account
    #[error("no account")]
    NoAccount,

    /// Client was not constructed with a signer
    #[error("Client was not constructed with a signer")]
    MissingSinger,

    #[error(transparent)]
    Metadata(#[from] MetadataError),
}

/// Representation of all the errors that can occur when building [`GrpcClient`] using
/// [`GrpcClientBuilder`]
///
/// [`GrpcClient`]: crate::GrpcClient
/// [`GrpcClientBuilder`]: crate::GrpcClientBuilder
#[derive(Debug, thiserror::Error)]
pub enum GrpcClientBuilderError {
    /// Error from tonic transport
    #[error(transparent)]
    #[cfg(not(target_arch = "wasm32"))]
    TonicTransportError(#[from] tonic::transport::Error),

    /// Transport has not been set for builder
    #[error("Transport not set")]
    TransportNotSet,

    /// Invalid private key.
    #[error("Invalid private key")]
    InvalidPrivateKey,

    /// Invalid public key.
    #[error("Invalid public key")]
    InvalidPublicKey,

    #[error(transparent)]
    Metadata(#[from] MetadataError),
}

#[derive(thiserror::Error, Debug)]
pub enum MetadataError {
    // is it okay to print it if it can store key or sth?
    #[error("Invalid metadata key ({0})")]
    Key(String),

    // is it okay to print it if it can store key or sth?
    #[error("Invalid binary metadata key ({0:?})")]
    KeyBin(Vec<u8>),

    // is it okay to print it if it can store key or sth?
    #[error("Invalid metadata value ({0:?})")]
    Value(String),
}

impl From<Status> for Error {
    fn from(value: Status) -> Self {
        Error::TonicError(Box::new(value))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
impl From<Error> for wasm_bindgen::JsValue {
    fn from(error: Error) -> wasm_bindgen::JsValue {
        error.to_string().into()
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
impl From<GrpcClientBuilderError> for wasm_bindgen::JsValue {
    fn from(error: GrpcClientBuilderError) -> wasm_bindgen::JsValue {
        error.to_string().into()
    }
}
