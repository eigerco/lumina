//! Compatibility layer for exporting gRPC functionality via uniffi

use std::sync::Arc;

use k256::ecdsa::VerifyingKey;
use uniffi::Object;

mod grpc_client;

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

    /// Tried to enable tls on pre-configured transport
    #[error("Cannot enable tls on manually configured transport")]
    CannotEnableTlsOnCustomTransport,

    /// Invalid account public key
    #[error("invalid account public key")]
    InvalidAccountPublicKey,

    /// Invalid account private key
    #[error("invalid account private key")]
    InvalidAccountPrivateKey,
}

/// Builder for [`GrpcClient`]
#[derive(Object)]
pub struct GrpcClientBuilder {
    url: String,
    signer: Option<Arc<dyn UniffiSigner>>,
    account_pubkey: Option<VerifyingKey>,
    tls: bool,
}

// note: we cannot use the GrpcClient::builder() returns GrpcClientBuilder
// pattern as in rust or js, because uniffi does not support static methods
// except for constructors: https://github.com/mozilla/uniffi-rs/issues/1074
#[uniffi::export(async_runtime = "tokio")]
impl GrpcClientBuilder {
    /// Create a new builder for the provided url
    #[uniffi::constructor(name = "withUrl")]
    pub fn with_url(url: String) -> Self {
        GrpcClientBuilder {
            url,
            signer: None,
            account_pubkey: None,
            tls: false,
        }
    }

    /// Add public key and signer to the client being built
    #[uniffi::method(name = "withPubkeyAndSigner")]
    pub fn pubkey_and_signer(
        self: Arc<Self>,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
    ) -> Result<Self, GrpcClientBuilderError> {
        let vk = VerifyingKey::from_sec1_bytes(&account_pubkey)
            .map_err(|_| GrpcClientBuilderError::InvalidAccountPublicKey)?;

        Ok(GrpcClientBuilder {
            url: self.url.clone(),
            signer: Some(signer),
            account_pubkey: Some(vk),
            tls: self.tls,
        })
    }

    /// Enables loading the certificate roots which were enabled by feature flags
    /// `tls-webpki-roots` and `tls-native-roots`.
    #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
    #[uniffi::method(name = "withDefaultTls")]
    pub fn default_tls(self: Arc<Self>) -> Self {
        GrpcClientBuilder {
            url: self.url.clone(),
            signer: self.signer.clone(),
            account_pubkey: self.account_pubkey,
            tls: true,
        }
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// Build the gRPC client.
    #[uniffi::method(name = "build")]
    pub async fn build(self: Arc<Self>) -> Result<GrpcClient, GrpcClientBuilderError> {
        let mut builder = crate::GrpcClientBuilder::new().url(self.url.clone());

        #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
        if self.tls {
            builder = builder.default_tls()?;
        }

        if let Some(signer) = self.signer.clone() {
            let signer = UniffiSignerBox(signer.clone());
            let vk = self.account_pubkey.expect("public key present");

            builder = builder.pubkey_and_signer(vk, signer);
        }

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
            crate::GrpcClientBuilderError::CannotEnableTlsOnCustomTransport => {
                GrpcClientBuilderError::CannotEnableTlsOnCustomTransport
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
        }
    }
}
