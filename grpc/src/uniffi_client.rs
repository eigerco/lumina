//! Compatibility layer for exporting gRPC functionality via uniffi

use std::sync::Arc;

use k256::ecdsa::VerifyingKey;
use uniffi::Object;

mod grpc_client;
mod tx_client;

use crate::builder::GrpcClientBuilder as RustBuilder;
use tx_client::{UniffiSigner, UniffiSignerBox};

pub use grpc_client::GrpcClient;
pub use tx_client::TxClient;

/// Errors returned when building Grpc Client
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcClientBuilderError {
    /// Error creating transport
    #[error("error creating transport: {msg}")]
    TransportCreationError {
        /// error message
        msg: String,
    },

    /// Invalid account public key
    #[error("invalid account public key")]
    InvalidAccountPublicKey {
        /// error message
        msg: String,
    },

    /// Missing account key and signer when creating [`TxClient`]
    #[error("missing required TxClient parameters")]
    MissingAccountKeyAndSigner,

    /// Error building client
    #[error("Error building client: {msg}")]
    ErrorBuildingClient {
        /// message
        msg: String,
    },

    /// Error connecting to endpoint
    #[error("Error connecting to the endpoint {msg}")]
    ErrorConnecting {
        /// message
        msg: String,
    },
}

/// Builder for creating either read [`GrpcClient`] or message sending capable [`TxClient`]
#[derive(Object)]
pub struct GrpcClientBuilder {
    url: String,
    signer: Option<Arc<dyn UniffiSigner>>,
    account_pubkey: Option<VerifyingKey>,
}

#[uniffi::export(async_runtime = "tokio")]
impl GrpcClientBuilder {
    /// Create a new builder for the provided url
    #[uniffi::constructor(name = "create")]
    pub fn new(url: String) -> Self {
        GrpcClientBuilder {
            url,
            signer: None,
            account_pubkey: None,
        }
    }

    /// Add public key and signer to the client being built
    #[uniffi::method(name = "withPubkeyAndSigner")]
    pub fn with_pubkey_and_signer(
        self: Arc<Self>,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
    ) -> Result<Self, GrpcClientBuilderError> {
        let vk = VerifyingKey::from_sec1_bytes(&account_pubkey)
            .map_err(|e| GrpcClientBuilderError::InvalidAccountPublicKey { msg: e.to_string() })?;

        Ok(GrpcClientBuilder {
            url: self.url.clone(),
            signer: Some(signer),
            account_pubkey: Some(vk),
        })
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// build gRPC read-only client. If you need to send messages, use [`build_tx_client`]
    ///
    /// [`build_tx_client`]: GrpcClientBuilder::build_tx_client
    #[uniffi::method(name = "buildClient")]
    pub async fn build_client(self: Arc<Self>) -> Result<GrpcClient, GrpcClientBuilderError> {
        Ok(RustBuilder::with_url(self.url.clone())?
            .connect()
            .map_err(|e| GrpcClientBuilderError::ErrorConnecting { msg: e.to_string() })?
            .build_client()
            .into())
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// build gRPC client capable of submitting messages, requires setting `with_pubkey_and_signer`
    #[uniffi::method(name = "buildTxClient")]
    pub async fn build_tx_client(self: Arc<Self>) -> Result<TxClient, GrpcClientBuilderError> {
        let account_pubkey = self
            .account_pubkey
            .ok_or(GrpcClientBuilderError::MissingAccountKeyAndSigner)?;

        let signer = UniffiSignerBox(
            self.signer
                .as_ref()
                .ok_or(GrpcClientBuilderError::MissingAccountKeyAndSigner)?
                .clone(),
        );

        Ok(RustBuilder::with_url(self.url.clone())?
            .with_pubkey_and_signer(account_pubkey, signer)
            .connect()
            .map_err(|e| GrpcClientBuilderError::ErrorConnecting { msg: e.to_string() })?
            .build_tx_client()
            .await
            .map_err(|e| GrpcClientBuilderError::ErrorBuildingClient { msg: e.to_string() })?
            .into())
    }
}

impl From<tonic::transport::Error> for GrpcClientBuilderError {
    fn from(error: tonic::transport::Error) -> GrpcClientBuilderError {
        GrpcClientBuilderError::TransportCreationError {
            msg: format!("Error creating transport: {error}"),
        }
    }
}
