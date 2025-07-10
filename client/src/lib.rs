//! A high-level client for interacting with a Celestia node.
//!
//! It combines the functionality of [`celestia-rpc`] and [`celestia-grpc`] crates.
//!
//! There are two modes: read-only mode and submit mode. Read-only mode requires
//! RPC endpoint and submit mode requires RPC/GRPC endpoints, and a signer.
//!
//! # Examples
//!
//! Read-only mode:
//!
//! ```no_run
//! # use celestia_client::Result;
//! # async fn docs() -> Result<()> {
//! let client = Client::builder()
//!     .rcp_url(RPC_URL)
//!     .await?;
//!
//! client.header().head().await?;
//! # }
//! ```
//!
//! Submit mode:
//!
//! ```no_run
//! # use celestia_client::Result;
//! # use k256::ecdsa::SigningKey;
//! # const RPC_URL: &str = "http://localhost:26658";
//! # const GRPC_URL : &str = "http://localhost:19090";
//! # async fn docs() -> Result<()> {
//! let client = Client::builder()
//!     .rcp_url(RPC_URL)
//!     .grpc_url(GRPC_URL)
//!     .plaintext_private_key("...")
//!     .await?;
//!
//! let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
//! client.state().transfer(to_address, 12345, TxConfig::default()).await?;
//! # }
//! ```
//!
//! [`celestia-rpc`]: celestia_rpc
//! [`celestia-grpc`]: celestia_grpc

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

mod blob;
mod blobstream;
mod client;
mod fraud;
mod header;
mod share;
mod state;
mod utils;

/// API related types.
pub mod api {
    pub use crate::blob::BlobApi;
    pub use crate::blobstream::BlobstreamApi;
    pub use crate::fraud::FraudApi;
    pub use crate::header::HeaderApi;
    pub use crate::share::ShareApi;
    pub use crate::state::StateApi;
}

/// TX related types.
pub mod tx {
    #[doc(inline)]
    pub use celestia_grpc::{DocSigner, IntoAny, TxConfig, TxInfo};
}

pub use crate::client::{Client, ClientBuilder};

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

    /// Celestia types error
    #[error("Celestia types error: {0}")]
    Types(#[from] celestia_types::Error),

    /// Client is in read-only mode
    #[error("Client is constructed for read-only mode")]
    ReadOnlyMode,

    /// Invalid height
    #[error("Invalid height: {0}")]
    InvalidHeight(u64),

    #[error("Invalid blob commitment")]
    InvalidBlobCommitment,

    /// Invalid private key
    #[error("Invalid private key")]
    InvalidPrivateKey,

    #[error("RPC endpoint not set")]
    RpcEndpointNotSet,

    #[error("GRPC endpoint is set but singer is not")]
    SignerNotSet,

    #[error("Signer is set but GRPC endpoint is not")]
    GrpcEndpointNotSet,
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
