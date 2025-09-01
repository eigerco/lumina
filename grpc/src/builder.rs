use std::error::Error as StdError;

use k256::ecdsa::VerifyingKey;
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::codegen::Service;

use crate::boxed::{boxed, AbstractBody, BoxedTransport, ConditionalSend};
use crate::client::SignerBits;
use crate::signer::DispatchedDocSigner;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

use imp::build_transport;

enum TransportSetup {
    Configuration { url: String, tls: bool },
    BoxedTransport(BoxedTransport),
}

/// Builder for [`GrpcClient`]
pub struct GrpcClientBuilder {
    transport: TransportSetup,
    signer_bits: Option<SignerBits>,
}

impl GrpcClientBuilder {
    /// Create a new client connected to the given `url` using [`Channel`] transport
    ///
    /// [`Channel`]: tonic::transport::Channel
    pub fn with_url(url: impl Into<String>) -> Self {
        GrpcClientBuilder {
            transport: TransportSetup::Configuration {
                url: url.into(),
                tls: false,
            },
            signer_bits: None,
        }
    }

    /// Create a gRPC client builder using provided prepared transport
    pub fn with_transport<B, T>(transport: T) -> Self
    where
        //B: Body<Data = Bytes> + Send + Sync + Unpin + 'static,
        B: http_body::Body + AbstractBody + Send + Unpin + 'static,
        <B as http_body::Body>::Error: StdError + Send + Sync,
        T: Service<http::Request<TonicBody>, Response = http::Response<B>>
            + Send
            + Sync
            + Clone
            + 'static,
        <T as Service<http::Request<TonicBody>>>::Error: StdError + Send + Sync + 'static,
        <T as Service<http::Request<TonicBody>>>::Future: ConditionalSend + 'static,
    {
        Self {
            transport: TransportSetup::BoxedTransport(boxed(transport)),
            signer_bits: None,
        }
    }

    /// Enables loading the certificate roots which were enabled by feature flags
    /// `tls-webpki-roots` and `tls-native-roots`.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
    pub fn with_default_tls(self) -> Result<Self, GrpcClientBuilderError> {
        let TransportSetup::Configuration { url, .. } = self.transport else {
            return Err(GrpcClientBuilderError::CannotEnableTlsOnManualTransport);
        };

        Ok(Self {
            transport: TransportSetup::Configuration { url, tls: true },
            ..self
        })
    }

    /// Add signer and a public key
    pub fn with_pubkey_and_signer<S>(
        self,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> GrpcClientBuilder
    where
        S: DocSigner + 'static,
    {
        GrpcClientBuilder {
            transport: self.transport,
            signer_bits: Some(SignerBits {
                signer: DispatchedDocSigner::new(signer),
                pubkey: account_pubkey,
            }),
        }
    }

    /// Add signer and associated public key
    pub fn with_signer_keypair<S>(self, signer: S) -> GrpcClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = signer.verifying_key();
        GrpcClientBuilder {
            transport: self.transport,
            signer_bits: Some(SignerBits {
                signer: DispatchedDocSigner::new(signer),
                pubkey,
            }),
        }
    }

    /// Build [`GrpcClient`]
    pub fn build(self) -> Result<GrpcClient, GrpcClientBuilderError> {
        let transport = match self.transport {
            TransportSetup::Configuration { url, tls } => build_transport(url, tls)?,
            TransportSetup::BoxedTransport(transport) => transport,
        };

        Ok(GrpcClient::new(transport, self.signer_bits))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
mod imp {
    use super::*;

    use tonic::transport::{ClientTlsConfig, Endpoint};

    pub(super) fn build_transport(
        url: String,
        tls: bool,
    ) -> Result<BoxedTransport, tonic::transport::Error> {
        let mut tls_config = ClientTlsConfig::new();

        #[cfg(feature = "tls-native-roots")]
        if tls {
            tls_config = tls_config.with_native_roots();
        }

        #[cfg(feature = "tls-webpki-roots")]
        if tls {
            tls_config = tls_config.with_webpki_roots();
        }

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

    pub(super) fn build_transport(
        url: String,
        _tls: bool,
    ) -> Result<BoxedTransport, tonic::transport::Error> {
        let channel = Endpoint::from_shared(url)?
            .user_agent("celestia-grpc")?
            .connect_lazy();

        Ok(boxed(channel))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    pub(super) fn build_transport(
        url: String,
        _tls: bool,
    ) -> Result<BoxedTransport, GrpcClientBuilderError> {
        Ok(boxed(tonic_web_wasm_client::Client::new(url)))
    }
}
