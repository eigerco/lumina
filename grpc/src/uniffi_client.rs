//! Compatibility layer for exporting gRPC functionality via uniffi

use std::sync::{Arc, Mutex};
use std::time::Duration;

use k256::ecdsa::VerifyingKey;
use uniffi::Object;

mod grpc_client;

use crate::EndpointConfig as RustEndpointConfig;
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

/// Configuration specific to a single URL endpoint.
///
/// This includes HTTP/2 headers (metadata) and timeout that will be applied
/// to all requests made to this endpoint.
#[derive(Object)]
pub struct EndpointConfig(Mutex<Option<RustEndpointConfig>>);

#[uniffi::export]
impl EndpointConfig {
    /// Create a new, empty endpoint configuration.
    #[uniffi::constructor]
    pub fn new() -> Self {
        EndpointConfig(Mutex::new(Some(RustEndpointConfig::new())))
    }

    /// Appends ASCII metadata (HTTP/2 header) to requests made to this endpoint.
    #[uniffi::method(name = "withMetadata")]
    pub fn metadata(self: Arc<Self>, key: &str, value: &str) -> Arc<Self> {
        {
            let mut lock = self.0.lock().expect("lock poisoned");
            let config = lock.take().expect("config must be set");
            *lock = Some(config.metadata(key, value));
        }
        self
    }

    /// Appends binary metadata to requests made to this endpoint.
    ///
    /// Keys must have `-bin` suffix.
    #[uniffi::method(name = "withMetadataBin")]
    pub fn metadata_bin(self: Arc<Self>, key: &str, value: &[u8]) -> Arc<Self> {
        {
            let mut lock = self.0.lock().expect("lock poisoned");
            let config = lock.take().expect("config must be set");
            *lock = Some(config.metadata_bin(key, value.to_vec()));
        }
        self
    }

    /// Sets the request timeout in milliseconds for this endpoint.
    #[uniffi::method(name = "withTimeout")]
    pub fn timeout(self: Arc<Self>, timeout_ms: u64) -> Arc<Self> {
        {
            let mut lock = self.0.lock().expect("lock poisoned");
            let config = lock.take().expect("config must be set");
            *lock = Some(config.timeout(Duration::from_millis(timeout_ms)));
        }
        self
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointConfig {
    fn take(&self) -> RustEndpointConfig {
        self.0
            .lock()
            .expect("lock poisoned")
            .take()
            .expect("config must be set")
    }
}

/// An endpoint entry containing a URL and its configuration.
///
/// Used to configure multiple endpoints with `GrpcClientBuilder.withUrls()`.
#[derive(Object)]
pub struct EndpointEntry {
    url: String,
    config: RustEndpointConfig,
}

#[uniffi::export]
impl EndpointEntry {
    /// Create a new endpoint entry with a URL and optional configuration.
    ///
    /// If no configuration is provided, default settings will be used.
    #[uniffi::constructor]
    pub fn new(url: String, config: Option<Arc<EndpointConfig>>) -> Self {
        Self {
            url,
            config: config.map(|c| c.take()).unwrap_or_default(),
        }
    }
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
    /// Create a new builder for the provided url.
    ///
    /// Example:
    /// ```swift
    /// let client = try await GrpcClientBuilder.withUrl(url: "http://localhost:9090").build()
    /// ```
    #[uniffi::constructor(name = "withUrl")]
    pub fn with_url(url: String) -> Self {
        let builder = RustBuilder::new().url(url);
        GrpcClientBuilder(Mutex::new(Some(builder)))
    }

    /// Create a new builder for the provided url with configuration.
    ///
    /// Example:
    /// ```swift
    /// let config = EndpointConfig().withMetadata(key: "authorization", value: "Bearer token")
    /// let client = try await GrpcClientBuilder.withUrlWithConfig(url: "http://localhost:9090", config: config).build()
    /// ```
    #[uniffi::constructor(name = "withUrlWithConfig")]
    pub fn with_url_with_config(url: String, config: Arc<EndpointConfig>) -> Self {
        let config = config.take();
        let builder = RustBuilder::new().url_with_config(url, config);
        GrpcClientBuilder(Mutex::new(Some(builder)))
    }

    /// Create a new builder with multiple fallback URLs.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// Example:
    /// ```swift
    /// let client = try await GrpcClientBuilder.withUrls(urls: ["http://primary:9090", "http://fallback:9090"]).build()
    /// ```
    #[uniffi::constructor(name = "withUrls")]
    pub fn with_urls(urls: Vec<String>) -> Self {
        let builder = RustBuilder::new().urls(urls);
        GrpcClientBuilder(Mutex::new(Some(builder)))
    }

    /// Create a new builder with multiple fallback URLs and their configurations.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// Example:
    /// ```swift
    /// let endpoints = [
    ///     EndpointEntry(url: "http://primary:9090", config: EndpointConfig().withTimeout(5000)),
    ///     EndpointEntry(url: "http://fallback:9090", config: nil)
    /// ]
    /// let client = try await GrpcClientBuilder.withUrlsAndConfig(endpoints: endpoints).build()
    /// ```
    #[uniffi::constructor(name = "withUrlsAndConfig")]
    pub fn with_urls_and_config(endpoints: Vec<Arc<EndpointEntry>>) -> Self {
        let urls_with_configs: Vec<(String, RustEndpointConfig)> = endpoints
            .into_iter()
            .map(|e| (e.url.clone(), e.config.clone()))
            .collect();
        let builder = RustBuilder::new().urls_with_config(urls_with_configs);
        GrpcClientBuilder(Mutex::new(Some(builder)))
    }

    /// Add another URL endpoint.
    #[uniffi::method(name = "addUrl")]
    pub fn add_url(self: Arc<Self>, url: String) -> Arc<Self> {
        self.map_builder(move |builder| builder.url(url));
        self
    }

    /// Add another URL endpoint with configuration.
    #[uniffi::method(name = "addUrlWithConfig")]
    pub fn add_url_with_config(
        self: Arc<Self>,
        url: String,
        config: Arc<EndpointConfig>,
    ) -> Arc<Self> {
        let config = config.take();
        self.map_builder(move |builder| builder.url_with_config(url, config));
        self
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
