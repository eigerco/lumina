use std::error::Error as StdError;
use std::fmt;

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

enum TransportEntry {
    EndpointUrl(String),
    BoxedTransport(BoxedTransport),
}

/// Builder for [`GrpcClient`]
///
/// Note that TLS configuration is governed using `tls-*-roots` feature flags.
#[derive(Default)]
pub struct GrpcClientBuilder {
    transports: Vec<TransportEntry>,
    signer_kind: Option<SignerKind>,
    ascii_metadata: Vec<(String, String)>,
    binary_metadata: Vec<(String, Vec<u8>)>,
    metadata_map: Option<MetadataMap>,
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

    /// Add a URL endpoint. Multiple calls add multiple fallback endpoints.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// [`Channel`]: tonic::transport::Channel
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.transports
            .push(TransportEntry::EndpointUrl(url.into()));
        self
    }

    /// Add a transport endpoint. Multiple calls add multiple fallback endpoints.
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
        self.transports
            .push(TransportEntry::BoxedTransport(boxed(transport, TransportMetadata::new())));
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

    /// Appends ascii metadata to all requests made by the client.
    pub fn metadata(mut self, key: &str, value: &str) -> GrpcClientBuilder {
        self.ascii_metadata.push((key.into(), value.into()));
        self
    }

    /// Appends binary metadata to all requests made by the client.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    pub fn metadata_bin(mut self, key: &str, value: &[u8]) -> GrpcClientBuilder {
        self.binary_metadata.push((key.into(), value.into()));
        self
    }

    /// Sets the initial metadata map that will be attached to all requestes made by the client.
    pub fn metadata_map(mut self, metadata: MetadataMap) -> GrpcClientBuilder {
        self.metadata_map = Some(metadata);
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
                TransportEntry::EndpointUrl(url) => build_transport(url),
                TransportEntry::BoxedTransport(t) => Ok(t),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let transports: std::sync::Arc<[BoxedTransport]> = transports.into();

        let signer_config = self.signer_kind.map(TryInto::try_into).transpose()?;

        let mut context = Context::default();
        for (key, value) in self.ascii_metadata {
            context.append_metadata(&key, &value)?;
        }
        for (key, value) in self.binary_metadata {
            context.append_metadata_bin(&key, &value)?;
        }
        if let Some(metadata) = self.metadata_map {
            context.append_metadata_map(&metadata);
        }

        Ok(GrpcClient::new(transports, signer_config, context))
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

    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, GrpcClientBuilderError> {
        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let channel = Endpoint::from_shared(url.clone())?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        Ok(boxed(channel, TransportMetadata::with_url(url)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
mod imp {
    use super::*;

    use tonic::transport::Endpoint;

    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, GrpcClientBuilderError> {
        if url
            .split_once(':')
            .is_some_and(|(scheme, _)| scheme == "https")
        {
            return Err(GrpcClientBuilderError::TlsNotSupported);
        }

        let channel = Endpoint::from_shared(url.clone())?
            .user_agent("celestia-grpc")?
            .connect_lazy();

        Ok(boxed(channel, TransportMetadata::with_url(url)))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, GrpcClientBuilderError> {
        let client = tonic_web_wasm_client::Client::new(url.clone());
        Ok(boxed(client, TransportMetadata::with_url(url)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_returns_transport_not_set() {
        let result = GrpcClientBuilder::new().build();

        assert!(matches!(
            result,
            Err(GrpcClientBuilderError::TransportNotSet)
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn single_url_builds_successfully() {
        let result = GrpcClientBuilder::new()
            .url("http://localhost:9090")
            .build();

        assert!(result.is_ok());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn multiple_urls_build_successfully() {
        let result = GrpcClientBuilder::new()
            .url("http://localhost:9090")
            .url("http://localhost:9091")
            .url("http://localhost:9092")
            .build();

        assert!(result.is_ok());
    }
}
