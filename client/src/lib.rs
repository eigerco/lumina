use std::sync::Arc;

use celestia_grpc::TxClient;
use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::{
    BlobClient, Client as RpcClient, DasClient, HeaderClient, ShareClient, StateClient,
};
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::state::Address;
use celestia_types::Commitment;
use celestia_types::{AppVersion, ExtendedHeader};
use tendermint::chain::Id;
use tendermint::crypto::default::ecdsa_secp256k1::VerifyingKey;

pub mod blob;
pub mod blobstream;
mod client;
pub mod fraud;
pub mod header;
pub mod share;
pub mod state;
mod utils;

pub mod tx {
    #[doc(inline)]
    pub use celestia_grpc::{DocSigner, IntoAny, TxConfig, TxInfo};
}

pub use crate::client::Client;

/// Alias for a `Result` with the error type [`celestia_client::Error`].
///
/// [`celestia_client::Error`]: crate::Error
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with [`celestia_client`].
///
/// [`celestia_client`]: crate
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// [`celestia_rpc::Error`]
    #[error("RPC error: {0}")]
    Rpc(#[from] celestia_rpc::Error),

    /// [`celestia_grpc::Error`]
    #[error("GRPC error: {0}")]
    Grpc(#[from] celestia_grpc::Error),

    /// Client is in read-only mode
    #[error("Client is constructed for read-only mode")]
    ReadOnlyMode,

    /// Invalid height
    #[error("Invalid height: {0}")]
    InvalidHeight(u64),

    /// Celestia types error
    #[error("Celestia types error: {0}")]
    Types(#[from] celestia_types::Error),

    #[error("Invalid blob commitment")]
    InvalidBlobCommitment,
}

impl From<jsonrpsee_core::ClientError> for Error {
    fn from(value: jsonrpsee_core::ClientError) -> Self {
        Error::Rpc(celestia_rpc::Error::JsonRpc(value))
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        jsonrpsee_core::ClientError::ParseError(value).into()
    }
}
