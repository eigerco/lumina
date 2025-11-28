//! Compatibility layer for exporting gRPC functionality via uniffi

use std::sync::{Arc, Mutex};
use std::time::Duration;

use k256::ecdsa::VerifyingKey;
use uniffi::Object;

mod grpc_client;

use crate::GrpcClientBuilder as RustBuilder;
use crate::signer::{UniffiSigner, UniffiSignerBox};

pub use grpc_client::GrpcClient;

/// Errors returned when building Grpc Client
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcClientBuilderError {
    /// Error creating transport
    #[error("error creating transport: {msg}")]
    TonicTransportError {
        /// error message
        msg: String,
    },

    /// Invalid account public key
    #[error("invalid account public key")]
    InvalidAccountPublicKey,

    /// Invalid account private key
    #[error("invalid account private key")]
    InvalidAccountPrivateKey,

    /// Invalid metadata
    #[error("invalid metadata")]
    Metadata(String),

    /// Tls support is not enabled but requested
    #[error(
        "Tls support is not enabled but requested via url, please enable it using proper feature flags"
    )]
    TlsNotSupported,
}

/// Builder for [`GrpcClient`]
#[derive(Object)]
pub struct GrpcClientBuilder(Mutex<Option<RustBuilder>>);

impl GrpcClientBuilder {
    /// Apply given transformation to the inner builder
    fn map_builder<F>(&self, map: F)
    where
        F: FnOnce(RustBuilder) -> RustBuilder,
    {
        let mut builder_lock = self.0.lock().expect("lock poisoned");
        let builder = builder_lock.take().expect("builder must be set");
        *builder_lock = Some(map(builder));
    }
}

// note: we cannot use the GrpcClient::builder() returns GrpcClientBuilder
// pattern as in rust or js, because uniffi does not support static methods
// except for constructors: https://github.com/mozilla/uniffi-rs/issues/1074
#[uniffi::export(async_runtime = "tokio")]
impl GrpcClientBuilder {
    /// Create a new builder for the provided url
    #[uniffi::constructor(name = "withUrl")]
    pub fn with_url(url: String) -> Self {
        let builder = RustBuilder::new().url(url);
        GrpcClientBuilder(Mutex::new(Some(builder)))
    }

    /// Add public key and signer to the client being built
    #[uniffi::method(name = "withPubkeyAndSigner")]
    pub fn pubkey_and_signer(
        self: Arc<Self>,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
    ) -> Result<Arc<Self>, GrpcClientBuilderError> {
        let vk = VerifyingKey::from_sec1_bytes(&account_pubkey)
            .map_err(|_| GrpcClientBuilderError::InvalidAccountPublicKey)?;
        let signer = UniffiSignerBox(signer);

        self.map_builder(move |builder| builder.pubkey_and_signer(vk, signer));

        Ok(self)
    }

    /// Appends ascii metadata to all requests made by the client.
    #[uniffi::method(name = "withMetadata")]
    pub fn metadata(self: Arc<Self>, key: &str, value: &str) -> Arc<Self> {
        self.map_builder(move |builder| builder.metadata(key, value));
        self
    }

    /// Appends binary metadata to all requests made by the client.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    #[uniffi::method(name = "withMetadataBin")]
    pub fn metadata_bin(self: Arc<Self>, key: &str, value: &[u8]) -> Arc<Self> {
        self.map_builder(move |builder| builder.metadata_bin(key, value));
        self
    }

    /// Sets the request timeout in milliseconds, overriding default one from the transport.
    #[uniffi::method(name = "withTimeout")]
    pub fn timeout(self: Arc<Self>, timeout_ms: u64) -> Arc<Self> {
        self.map_builder(move |builder| builder.timeout(Duration::from_millis(timeout_ms)));
        self
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// Build the gRPC client.
    #[uniffi::method(name = "build")]
    pub async fn build(self: Arc<Self>) -> Result<GrpcClient, GrpcClientBuilderError> {
        let builder = self
            .0
            .lock()
            .expect("lock poisoned")
            .take()
            .expect("builder must be set");

        Ok(builder.build()?.into())
    }
}

impl From<crate::GrpcClientBuilderError> for GrpcClientBuilderError {
    fn from(error: crate::GrpcClientBuilderError) -> Self {
        match error {
            crate::GrpcClientBuilderError::TonicTransportError(error) => {
                GrpcClientBuilderError::TonicTransportError {
                    msg: error.to_string(),
                }
            }
            crate::GrpcClientBuilderError::InvalidPrivateKey => {
                GrpcClientBuilderError::InvalidAccountPrivateKey
            }
            crate::GrpcClientBuilderError::InvalidPublicKey => {
                GrpcClientBuilderError::InvalidAccountPublicKey
            }
            crate::GrpcClientBuilderError::TransportNotSet => {
                // API above should not allow creating a builder without any transport
                unimplemented!("transport not set for builder, should not happen")
            }
            crate::GrpcClientBuilderError::Metadata(err) => {
                GrpcClientBuilderError::Metadata(err.to_string())
            }
            crate::GrpcClientBuilderError::TlsNotSupported => {
                GrpcClientBuilderError::TlsNotSupported
            }
        }
    }
}
