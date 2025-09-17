use std::error::Error as StdError;
use std::fmt;

use bytes::Bytes;
use k256::ecdsa::{SigningKey, VerifyingKey};
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::codegen::Service;
use zeroize::Zeroizing;

use crate::boxed::{boxed, BoxedTransport};
use crate::client::SignerConfig;
use crate::signer::BoxedDocSigner;
use crate::utils::CondSend;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

use imp::build_transport;

#[derive(Default)]
enum TransportSetup {
    #[default]
    Unset,
    EndpointUrl(String),
    BoxedTransport(BoxedTransport),
}

/// Builder for [`GrpcClient`]
///
/// Note that TLS configuration is governed using `tls-*-roots` feature flags.
#[derive(Default)]
pub struct GrpcClientBuilder {
    transport: TransportSetup,
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

    /// Set the `url` to connect to using [`Channel`] transport.
    ///
    /// [`Channel`]: tonic::transport::Channel
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.transport = TransportSetup::EndpointUrl(url.into());
        self
    }

    /// Create a gRPC client builder using provided prepared transport
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
        self.transport = TransportSetup::BoxedTransport(boxed(transport));
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
        self.signer_bits = Some(SignerKind::Signer((account_pubkey, signer)));
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
        self.signer_bits = Some(SignerKind::PrivKeyBytes(Zeroizing::new(bytes.to_vec())));
        self
    }

    /// Set signer from a hex formatted private key.
    pub fn private_key_hex(mut self, s: &str) -> GrpcClientBuilder {
        self.signer_bits = Some(SignerKind::PrivKeyHex(Zeroizing::new(s.to_string())));
        self
    }

    /// Build [`GrpcClient`]
    pub fn build(self) -> Result<GrpcClient, GrpcClientBuilderError> {
        let transport = match self.transport {
            TransportSetup::EndpointUrl(url) => build_transport(url)?,
            TransportSetup::BoxedTransport(transport) => transport,
            TransportSetup::Unset => return Err(GrpcClientBuilderError::TransportNotSet),
        };

        let signer_config = self.signer_bits.map(TryInto::try_into).transpose()?;

        Ok(GrpcClient::new(transport, signer_config))
    }
}

impl TryFrom<SignerKind> for SignerConfig {
    type Error = GrpcClientBuilderError;
    fn try_from(value: SignerKind) -> Result<Self, Self::Error> {
        match value {
            SignerKind::Signer((pubkey, signer)) => Ok(SignerConfig { signer, pubkey }),
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

fn priv_key_signer(bytes: &[u8]) -> Result<SignerConfig, GrpcClientBuilderError> {
    let signing_key =
        SigningKey::from_slice(bytes).map_err(|_| GrpcClientBuilderError::InvalidPrivateKey)?;
    let pubkey = signing_key.verifying_key().to_owned();
    let signer = BoxedDocSigner::new(signing_key);
    Ok(SignerConfig { signer, pubkey })
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

    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, tonic::transport::Error> {
        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let channel = Endpoint::from_shared(url)?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        Ok(boxed(channel))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
mod imp {
    use super::*;

    use tonic::transport::Endpoint;

    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, tonic::transport::Error> {
        let channel = Endpoint::from_shared(url)?
            .user_agent("celestia-grpc")?
            .connect_lazy();

        Ok(boxed(channel))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, GrpcClientBuilderError> {
        Ok(boxed(tonic_web_wasm_client::Client::new(url)))
    }
}
