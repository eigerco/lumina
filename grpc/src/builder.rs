use arc_swap::ArcSwap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use k256::ecdsa::{SigningKey, VerifyingKey};
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::codegen::Service;
use tonic::metadata::MetadataMap;
use zeroize::Zeroizing;

use crate::boxed::{BoxedTransport, TransportMetadata, boxed};
use crate::client::AccountState;
use crate::grpc::Context;
use crate::signer::BoxedDocSigner;
use crate::utils::CondSend;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

use imp::build_transport;

/// Configuration specific to a single URL endpoint.
///
/// This includes HTTP/2 headers (metadata) and timeout that will be applied
/// to all requests made to this endpoint.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use celestia_grpc::{GrpcClient, EndpointConfig};
///
/// let client = GrpcClient::builder()
///     .url("http://localhost:9090", EndpointConfig::new()
///         .metadata("authorization", "Bearer token")
///         .timeout(Duration::from_secs(30)))
///     .build();
/// ```
#[derive(Debug, Default, Clone)]
pub struct EndpointConfig {
    ascii_metadata: Vec<(String, String)>,
    binary_metadata: Vec<(String, Vec<u8>)>,
    metadata_map: Option<MetadataMap>,
    timeout: Option<Duration>,
}

impl EndpointConfig {
    /// Create a new, empty endpoint configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends ASCII metadata (HTTP/2 header) to requests made to this endpoint.
    ///
    /// Use this for auth tokens, request IDs, and other text headers.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.ascii_metadata.push((key.into(), value.into()));
        self
    }

    /// Appends binary metadata to requests made to this endpoint.
    ///
    /// Keys must have `-bin` suffix. Values are base64-encoded on the wire.
    /// Use this for signatures, serialized protos, and other binary data.
    pub fn metadata_bin(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.binary_metadata.push((key.into(), value.into()));
        self
    }

    /// Sets a metadata map for this endpoint.
    ///
    /// Use this when you have a pre-built MetadataMap to attach.
    pub fn metadata_map(mut self, metadata: MetadataMap) -> Self {
        self.metadata_map = Some(metadata);
        self
    }

    /// Sets the request timeout for this endpoint.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl TryFrom<EndpointConfig> for Context {
    type Error = GrpcClientBuilderError;

    fn try_from(config: EndpointConfig) -> Result<Self, Self::Error> {
        let mut context = Context {
            timeout: config.timeout,
            ..Default::default()
        };
        for (key, value) in config.ascii_metadata {
            context.append_metadata(&key, &value)?;
        }
        for (key, value) in config.binary_metadata {
            context.append_metadata_bin(&key, &value)?;
        }
        if let Some(metadata) = config.metadata_map {
            context.append_metadata_map(&metadata);
        }
        Ok(context)
    }
}

enum TransportEntry {
    EndpointUrl(String, EndpointConfig),
    BoxedTransport(BoxedTransport),
}

/// Builder for [`GrpcClient`]
///
/// Note that TLS configuration is governed using `tls-*-roots` feature flags.
#[derive(Default)]
pub struct GrpcClientBuilder {
    transports: Vec<TransportEntry>,
    signer_kind: Option<SignerKind>,
}

enum SignerKind {
    Signer((VerifyingKey, BoxedDocSigner)),
    PrivKeyBytes(Zeroizing<Vec<u8>>),
    PrivKeyHex(Zeroizing<String>),
}

impl GrpcClientBuilder {
    /// Create a new, empty builder.
    pub fn new() -> Self {
        GrpcClientBuilder::default()
    }

    /// Add multiple URL endpoints with their configurations.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use celestia_grpc::{GrpcClient, EndpointConfig};
    ///
    /// let client = GrpcClient::builder()
    ///     .urls([
    ///         ("http://primary:9090", EndpointConfig::new()
    ///             .metadata("auth", "token1")
    ///             .timeout(Duration::from_secs(5))),
    ///         ("http://fallback:9090", EndpointConfig::new()
    ///             .metadata("auth", "token2")),
    ///     ])
    ///     .build();
    /// ```
    pub fn urls<S: AsRef<str>>(
        mut self,
        urls: impl IntoIterator<Item = (S, EndpointConfig)>,
    ) -> Self {
        for (url, config) in urls {
            self = self.url(url.as_ref(), config);
        }
        self
    }

    /// Add a URL endpoint with specific configuration. Multiple calls add multiple fallback endpoints.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use celestia_grpc::{GrpcClient, EndpointConfig};
    ///
    /// let client = GrpcClient::builder()
    ///     .url("http://primary:9090", EndpointConfig::new()
    ///         .metadata("authorization", "Bearer token1")
    ///         .timeout(Duration::from_secs(5)))
    ///     .url("http://fallback:9090", EndpointConfig::new()
    ///         .metadata("authorization", "Bearer token2")
    ///         .timeout(Duration::from_secs(10)))
    ///     .build();
    /// ```
    pub fn url(mut self, url: impl Into<String>, config: EndpointConfig) -> Self {
        self.transports
            .push(TransportEntry::EndpointUrl(url.into(), config));
        self
    }

    /// Add a custom transport endpoint. Multiple calls add multiple fallback endpoints.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    pub fn transport<B, T>(mut self, transport: T) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
        <B as http_body::Body>::Error: StdError + Send + Sync,
        T: Service<http::Request<TonicBody>, Response = http::Response<B>>
            + Send
            + Sync
            + Clone
            + 'static,
        <T as Service<http::Request<TonicBody>>>::Error: StdError + Send + Sync + 'static,
        <T as Service<http::Request<TonicBody>>>::Future: CondSend + 'static,
    {
        self.transports.push(TransportEntry::BoxedTransport(boxed(
            transport,
            TransportMetadata::default(),
        )));
        self
    }

    /// Add signer and a public key
    pub fn pubkey_and_signer<S>(
        mut self,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> GrpcClientBuilder
    where
        S: DocSigner + 'static,
    {
        let signer = BoxedDocSigner::new(signer);
        self.signer_kind = Some(SignerKind::Signer((account_pubkey, signer)));
        self
    }

    /// Add signer and associated public key
    pub fn signer_keypair<S>(self, signer: S) -> GrpcClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = signer.verifying_key();
        self.pubkey_and_signer(pubkey, signer)
    }

    /// Set signer from a raw private key.
    pub fn private_key(mut self, bytes: &[u8]) -> GrpcClientBuilder {
        self.signer_kind = Some(SignerKind::PrivKeyBytes(Zeroizing::new(bytes.to_vec())));
        self
    }

    /// Set signer from a hex formatted private key.
    pub fn private_key_hex(mut self, s: &str) -> GrpcClientBuilder {
        self.signer_kind = Some(SignerKind::PrivKeyHex(Zeroizing::new(s.to_string())));
        self
    }

    /// Build [`GrpcClient`]
    ///
    /// Returns error if no transports were configured.
    pub fn build(self) -> Result<GrpcClient, GrpcClientBuilderError> {
        if self.transports.is_empty() {
            return Err(GrpcClientBuilderError::TransportNotSet);
        }

        let transports: Vec<BoxedTransport> = self
            .transports
            .into_iter()
            .map(|entry| match entry {
                TransportEntry::EndpointUrl(url, config) => {
                    let context = config.try_into()?;
                    build_transport(url, context)
                }
                TransportEntry::BoxedTransport(t) => Ok(t),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let transports = Arc::new(ArcSwap::from_pointee(transports));

        let signer_config = self.signer_kind.map(TryInto::try_into).transpose()?;

        Ok(GrpcClient::new(transports, signer_config))
    }
}

impl TryFrom<SignerKind> for AccountState {
    type Error = GrpcClientBuilderError;

    fn try_from(value: SignerKind) -> Result<Self, Self::Error> {
        match value {
            SignerKind::Signer((pubkey, signer)) => Ok(AccountState::new(pubkey, signer)),
            SignerKind::PrivKeyBytes(bytes) => priv_key_signer(&bytes),
            SignerKind::PrivKeyHex(string) => {
                let bytes = Zeroizing::new(
                    hex::decode(string.trim())
                        .map_err(|_| GrpcClientBuilderError::InvalidPrivateKey)?,
                );
                priv_key_signer(&bytes)
            }
        }
    }
}

fn priv_key_signer(bytes: &[u8]) -> Result<AccountState, GrpcClientBuilderError> {
    let signing_key =
        SigningKey::from_slice(bytes).map_err(|_| GrpcClientBuilderError::InvalidPrivateKey)?;
    let pubkey = signing_key.verifying_key().to_owned();
    let signer = BoxedDocSigner::new(signing_key);
    Ok(AccountState::new(pubkey, signer))
}

impl fmt::Debug for SignerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SignerKind::Signer(..) => "SignerKind::Signer(..)",
            SignerKind::PrivKeyBytes(..) => "SignerKind::PrivKeyBytes(..)",
            SignerKind::PrivKeyHex(..) => "SignerKind::PrivKeyHex(..)",
        };
        f.write_str(s)
    }
}

impl fmt::Debug for GrpcClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GrpcClientBuilder { .. }")
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
mod imp {
    use super::*;

    use tonic::transport::{ClientTlsConfig, Endpoint};

    pub(super) fn build_transport(
        url: String,
        context: Context,
    ) -> Result<BoxedTransport, GrpcClientBuilderError> {
        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let channel = Endpoint::from_shared(url.clone())?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        Ok(boxed(
            channel,
            TransportMetadata::with_url_and_context(url, context),
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
mod imp {
    use super::*;

    use tonic::transport::Endpoint;

    pub(super) fn build_transport(
        url: String,
        context: Context,
    ) -> Result<BoxedTransport, GrpcClientBuilderError> {
        if url
            .split_once(':')
            .is_some_and(|(scheme, _)| scheme == "https")
        {
            return Err(GrpcClientBuilderError::TlsNotSupported);
        }

        let channel = Endpoint::from_shared(url.clone())?
            .user_agent("celestia-grpc")?
            .connect_lazy();

        Ok(boxed(
            channel,
            TransportMetadata::with_url_and_context(url, context),
        ))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    pub(super) fn build_transport(
        url: String,
        context: Context,
    ) -> Result<BoxedTransport, GrpcClientBuilderError> {
        let client = tonic_web_wasm_client::Client::new(url.clone());
        Ok(boxed(
            client,
            TransportMetadata::with_url_and_context(url, context),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_utils::test_utils::async_test;

    #[test]
    fn empty_builder_returns_transport_not_set() {
        let result = GrpcClientBuilder::new().build();

        assert!(matches!(
            result,
            Err(GrpcClientBuilderError::TransportNotSet)
        ));
    }

    #[async_test]
    async fn single_url_builds_successfully() {
        let result = GrpcClientBuilder::new()
            .url("http://localhost:9090", EndpointConfig::default())
            .build();

        assert!(result.is_ok());
    }

    #[async_test]
    async fn multiple_urls_build_successfully() {
        let result = GrpcClientBuilder::new()
            .url("http://localhost:9090", EndpointConfig::default())
            .url("http://localhost:9091", EndpointConfig::default())
            .url("http://localhost:9092", EndpointConfig::default())
            .build();

        assert!(result.is_ok());
    }

    #[async_test]
    async fn url_with_metadata_builds_successfully() {
        let result = GrpcClientBuilder::new()
            .url(
                "http://localhost:9090",
                EndpointConfig::new()
                    .metadata("authorization", "Bearer token")
                    .timeout(Duration::from_secs(30)),
            )
            .build();

        assert!(result.is_ok());
    }

    #[async_test]
    async fn urls_helper_builds_successfully() {
        let result = GrpcClientBuilder::new()
            .urls([
                ("http://localhost:9090", EndpointConfig::default()),
                ("http://localhost:9091", EndpointConfig::default()),
            ])
            .build();

        assert!(result.is_ok());
    }
}
