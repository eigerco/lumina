use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use derive_builder::Builder;
use k256::ecdsa::{SigningKey, VerifyingKey};
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::codegen::Service;
use tonic::metadata::MetadataMap;
use zeroize::Zeroizing;

use crate::boxed::{BoxedTransport, boxed};
use crate::client::AccountState;
use crate::grpc::Context;
use crate::signer::BoxedDocSigner;
use crate::utils::CondSend;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

use imp::build_transport;

#[derive(Builder)]
#[builder(name = "GrpcClientBuilder", pattern = "owned")]
#[builder(build_fn(private, name = "build_partial", error = "GrpcClientBuilderError"))]
/// Builder for [`GrpcClient`]
///
/// Note that TLS configuration is governed using `tls-*-roots` feature flags.
pub struct GrpcClientPartialBuilder {
    #[builder(setter(custom))]
    transport: TransportSetup,

    /// Sets the request timeout, overriding default one from the transport
    #[builder(setter(strip_option))]
    timeout: Option<Duration>,

    #[builder(setter(custom))]
    signer_kind: Option<SignerKind>,

    #[builder(setter(custom), field(ty = "Vec<(String, String)>"))]
    ascii_metadata: Vec<(String, String)>,

    #[builder(setter(custom), field(ty = "Vec<(String, Vec<u8>)>"))]
    binary_metadata: Vec<(String, Vec<u8>)>,

    /// Sets the initial metadata map that will be attached to all the requests made by the client.
    #[builder(default)]
    metadata_map: MetadataMap,
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
        let _ = self
            .transport
            .insert(TransportSetup::EndpointUrl(url.into()));
        self
    }

    /// Sets the request timeout, overriding default one from the transport
    pub fn maybe_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = Some(timeout);
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
        let _ = self
            .transport
            .insert(TransportSetup::BoxedTransport(boxed(transport)));
        self
    }

    /// Add signer and a public key
    pub fn pubkey_and_signer<S>(mut self, account_pubkey: VerifyingKey, signer: S) -> Self
    where
        S: DocSigner + 'static,
    {
        let signer = BoxedDocSigner::new(signer);
        let _ = self
            .signer_kind
            .insert(Some(SignerKind::Signer((account_pubkey, signer))));
        self
    }

    /// Add signer and associated public key
    pub fn signer_keypair<S>(self, signer: S) -> Self
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = signer.verifying_key();
        self.pubkey_and_signer(pubkey, signer)
    }

    /// Set signer from a raw private key.
    pub fn private_key(mut self, bytes: &[u8]) -> Self {
        let _ = self
            .signer_kind
            .insert(Some(SignerKind::PrivKeyBytes(Zeroizing::new(
                bytes.to_vec(),
            ))));
        self
    }

    /// Set signer from a hex formatted private key.
    pub fn private_key_hex(mut self, s: &str) -> Self {
        let _ = self
            .signer_kind
            .insert(Some(SignerKind::PrivKeyHex(Zeroizing::new(s.to_string()))));
        self
    }

    /// Appends ASCII metadata to all requests made by the client.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.ascii_metadata.push((key.into(), value.into()));
        self
    }

    /// Appends binary metadata to all requests made by the client.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    pub fn metadata_bin(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.binary_metadata.push((key.into(), value.into()));
        self
    }

    pub fn build(self) -> Result<GrpcClient, GrpcClientBuilderError> {
        let GrpcClientPartialBuilder {
            transport,
            timeout,
            signer_kind,
            ascii_metadata,
            binary_metadata,
            metadata_map,
        } = self.build_partial()?;

        let mut context = Context {
            timeout,
            metadata: metadata_map,
        };
        for (key, value) in ascii_metadata {
            context.append_metadata(&key, &value)?;
        }
        for (key, value) in binary_metadata {
            context.append_metadata_bin(&key, &value)?;
        }

        Ok(GrpcClient::new(
            transport.try_into()?,
            signer_kind.map(TryInto::try_into).transpose()?,
            context,
        ))
    }
}

enum TransportSetup {
    EndpointUrl(String),
    BoxedTransport(BoxedTransport),
}

impl TryFrom<TransportSetup> for BoxedTransport {
    type Error = GrpcClientBuilderError;

    fn try_from(value: TransportSetup) -> Result<Self, Self::Error> {
        Ok(match value {
            TransportSetup::EndpointUrl(url) => build_transport(url)?,
            TransportSetup::BoxedTransport(transport) => transport,
        })
    }
}

enum SignerKind {
    Signer((VerifyingKey, BoxedDocSigner)),
    PrivKeyBytes(Zeroizing<Vec<u8>>),
    PrivKeyHex(Zeroizing<String>),
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

    pub(super) fn build_transport(url: String) -> Result<BoxedTransport, GrpcClientBuilderError> {
        if url
            .split_once(':')
            .is_some_and(|(scheme, _)| scheme == "https")
        {
            return Err(GrpcClientBuilderError::TlsNotSupported);
        }

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
