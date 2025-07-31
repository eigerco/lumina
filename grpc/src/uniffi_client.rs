//! Compatibility layer for exporting gRPC functionality via uniffi

use std::sync::Arc;

use k256::ecdsa::VerifyingKey;
use uniffi::Object;

mod grpc_client;

use crate::builder::NativeTransportBits;
use crate::signer::{UniffiSigner, UniffiSignerBox};

pub use grpc_client::GrpcClient;

type RustBuilder = crate::builder::GrpcClientBuilder<NativeTransportBits>;

/// Errors returned when building Grpc Client
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcClientBuilderError {
    /// Error creating transport
    #[error("error creating transport: {msg}")]
    TonicTransportError {
        /// error message
        msg: String,
    },

    /// Error handling certificate root
    #[error("webpki error: {msg}")]
    Webpki {
        /// error message
        msg: String,
    },

    /// Could not import system certificates
    #[error("Could not import platform certificates: {errors:?}")]
    RustlsNativeCerts {
        /// error messages
        errors: Vec<String>,
    },

    /// Invalid account public key
    #[error("invalid account public key")]
    InvalidAccountPublicKey {
        /// error message
        msg: String,
    },
}

/// Builder for creating either read [`GrpcClient`] or message sending capable [`TxClient`]
#[derive(Object)]
pub struct GrpcClientBuilder {
    url: String,
    signer: Option<Arc<dyn UniffiSigner>>,
    account_pubkey: Option<VerifyingKey>,
    native_roots: bool,
    webpki_roots: bool,
}

#[uniffi::export(async_runtime = "tokio")]
impl GrpcClientBuilder {
    /// Create a new builder for the provided url
    #[uniffi::constructor(name = "withUrl")]
    pub fn with_url(url: String) -> Self {
        GrpcClientBuilder {
            url,
            signer: None,
            account_pubkey: None,
            native_roots: false,
            webpki_roots: false,
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

        //Ok(Self { inner: self.inner.with_pubkey_and_signer(vk, todo!()) //signer) })
        Ok(GrpcClientBuilder {
            url: self.url.clone(),
            signer: Some(signer),
            account_pubkey: Some(vk),
            native_roots: self.native_roots,
            webpki_roots: self.webpki_roots,
        })
    }

    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    /// Build the gRPC client.
    #[uniffi::method(name = "build")]
    pub async fn build(self: Arc<Self>) -> Result<GrpcClient, GrpcClientBuilderError> {
        let mut builder = RustBuilder::with_url(self.url.clone());

        #[cfg(feature = "tls-native-roots")]
        if self.native_roots {
            builder = builder.with_native_roots();
        }

        #[cfg(feature = "tls-webpki-roots")]
        if self.webpki_roots {
            builder = builder.with_webpki_roots();
        }

        if let Some(signer) = self.signer.clone() {
            let signer = UniffiSignerBox(signer.clone());
            let vk = self.account_pubkey.expect("public key present");

            builder = builder.with_pubkey_and_signer(vk, signer);
        }

        Ok(builder.build()?.into())
    }
}

#[cfg(feature = "tls-native-roots")]
#[uniffi::export]
impl GrpcClientBuilder {
    /// Enable the platform trusted certs.
    #[uniffi::method(name = "withNativeRoots")]
    pub fn with_native_roots(self: Arc<Self>) -> Self {
        GrpcClientBuilder {
            url: self.url.clone(),
            signer: self.signer.clone(),
            account_pubkey: self.account_pubkey,
            native_roots: true,
            webpki_roots: self.webpki_roots,
        }
    }
}

#[cfg(feature = "tls-webpki-roots")]
#[uniffi::export]
impl GrpcClientBuilder {
    /// Enable webpki roots.
    #[uniffi::method(name = "withWebpkiRoots")]
    pub fn with_webpki_roots(self: Arc<Self>) -> Self {
        GrpcClientBuilder {
            url: self.url.clone(),
            signer: self.signer.clone(),
            account_pubkey: self.account_pubkey,
            native_roots: self.native_roots,
            webpki_roots: true,
        }
    }
}

impl From<crate::builder::GrpcClientBuilderError> for GrpcClientBuilderError {
    fn from(error: crate::builder::GrpcClientBuilderError) -> Self {
        match error {
            crate::builder::GrpcClientBuilderError::Webpki(error) => {
                GrpcClientBuilderError::Webpki {
                    msg: error.to_string(),
                }
            }
            crate::builder::GrpcClientBuilderError::RustlsNativeCerts { errors } => {
                GrpcClientBuilderError::RustlsNativeCerts {
                    errors: errors.into_iter().map(|e| e.to_string()).collect(),
                }
            }
            crate::builder::GrpcClientBuilderError::TonicTransportError(error) => {
                GrpcClientBuilderError::TonicTransportError {
                    msg: error.to_string(),
                }
            }
        }
    }
}
