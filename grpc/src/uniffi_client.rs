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
        })
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// Build the gRPC client.
    #[uniffi::method(name = "build")]
    pub async fn build(self: Arc<Self>) -> Result<GrpcClient, GrpcClientBuilderError> {
        let mut builder = crate::GrpcClientBuilder::new().url(self.url.clone());

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
            _ => todo!(),
        }
    }
}
