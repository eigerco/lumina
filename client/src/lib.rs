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
//! # use celestia_client::{Client, Result};
//! # const RPC_URL: &str = "http://localhost:26658";
//! # async fn docs() -> Result<()> {
//! let client = Client::builder()
//!     .rpc_url(RPC_URL)
//!     .build()
//!     .await?;
//!
//! client.header().head().await?;
//! # Ok(())
//! # }
//! ```
//!
//! Submit mode:
//!
//! ```no_run
//! # use celestia_client::{Client, Result};
//! # use celestia_client::tx::TxConfig;
//! # const RPC_URL: &str = "http://localhost:26658";
//! # const GRPC_URL : &str = "http://localhost:19090";
//! # async fn docs() -> Result<()> {
//! let client = Client::builder()
//!     .rpc_url(RPC_URL)
//!     .grpc_url(GRPC_URL)
//!     .plaintext_private_key("...")?
//!     .build()
//!     .await?;
//!
//! let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
//! client.state().transfer(&to_address, 12345, TxConfig::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [`celestia-rpc`]: celestia_rpc
//! [`celestia-grpc`]: celestia_grpc

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

    /// Blob API related types.
    pub mod blob {
        #[doc(inline)]
        pub use crate::blob::BlobsAtHeight;
    }

    /// Share API related types.
    pub mod share {
        #[doc(inline)]
        pub use crate::share::{GetRangeResponse, GetRowResponse, RowSide, SampleCoordinates};
    }
}

/// TX related types.
pub mod tx {
    #[doc(inline)]
    pub use celestia_grpc::{DocSigner, IntoProtobufAny, SignDoc, TxConfig, TxInfo};
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
    /// Celestia RPC error.
    #[error("RPC error: {0}")]
    Rpc(#[from] celestia_rpc::Error),

    /// Celestia GRPC error.
    #[error("GRPC error: {0}")]
    Grpc(#[from] celestia_grpc::Error),

    /// Celestia types error.
    #[error("Celestia types error: {0}")]
    Types(#[from] celestia_types::Error),

    /// Client is in read-only mode.
    #[error("Client is constructed for read-only mode")]
    ReadOnlyMode,

    /// RPC authentication token is not supported.
    #[error("RPC authentication token is not supported")]
    AuthTokenNotSupported,

    /// Invalid height.
    #[error("Invalid height: {0}")]
    InvalidHeight(u64),

    /// Invalid commitment in blob.
    #[error("Invalid blob commitment")]
    InvalidBlobCommitment,

    /// Invalid private key.
    #[error("Invalid private key")]
    InvalidPrivateKey,

    /// RPC endpoint is not set.
    #[error("RPC endpoint not set")]
    RpcEndpointNotSet,

    /// Signer is not set.
    #[error("GRPC endpoint is set but singer is not")]
    SignerNotSet,

    /// GRPC endpoint is not set.
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
