use std::error::Error as StdError;

use bytes::Bytes;
use http_body::Body;
use k256::ecdsa::VerifyingKey;
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::client::GrpcService;
use tonic::codegen::Service;
#[cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "tls-native-roots", feature = "tls-webpki-roots")
))]
use tonic::transport::ClientTlsConfig;
#[cfg(not(target_arch = "wasm32"))]
use tonic::transport::{Channel, Endpoint};

use crate::client::SignerBits;
use crate::signer::DispatchedDocSigner;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeTransportBits {
    url: String,
    tls: bool,
}

/// Builder for [`GrpcClient`]
pub struct GrpcClientBuilder<T> {
    transport_setup: T,
    signer_bits: Option<SignerBits>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GrpcClientBuilder<NativeTransportBits> {
    /// Create a new client connected to the given `url` using [`Channel`] transport
    ///
    /// [`Channel`]: tonic::transport::Channel
    pub fn with_url(url: impl Into<String>) -> Self {
        GrpcClientBuilder {
            transport_setup: NativeTransportBits {
                url: url.into(),
                tls: false,
            },
            signer_bits: None,
        }
    }

    /// Enables loading the certificate roots which were enabled by feature flags
    /// `tls-webpki-roots` and `tls-native-roots`.
    #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
    pub fn with_default_tls(self) -> Self {
        Self {
            transport_setup: NativeTransportBits {
                url: self.transport_setup.url,
                tls: true,
            },
            ..self
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl GrpcClientBuilder<tonic_web_wasm_client::Client> {
    /// Create a new client connected to the given grpc-web `url` with default
    /// settings of [`tonic_web_wasm_client::Client`].
    pub fn with_url(url: impl Into<String>) -> Self {
        let connection = tonic_web_wasm_client::Client::new(url.into());
        GrpcClientBuilder {
            transport_setup: connection,
            signer_bits: None,
        }
    }
}

impl<T> GrpcClientBuilder<T> {
    /// Create a gRPC client builder using provided prepared transport
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport_setup: transport,
            signer_bits: None,
        }
    }

    /// Add signer and a public key
    pub fn with_pubkey_and_signer<S>(
        self,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> GrpcClientBuilder<T>
    where
        S: DocSigner + 'static,
    {
        GrpcClientBuilder {
            transport_setup: self.transport_setup,
            signer_bits: Some(SignerBits {
                signer: DispatchedDocSigner::new(signer),
                pubkey: account_pubkey,
            }),
        }
    }

    /// Add signer and associated public key
    pub fn with_signer_keypair<S>(self, signer: S) -> GrpcClientBuilder<T>
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = signer.verifying_key();
        GrpcClientBuilder {
            transport_setup: self.transport_setup,
            signer_bits: Some(SignerBits {
                signer: DispatchedDocSigner::new(signer),
                pubkey,
            }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl GrpcClientBuilder<NativeTransportBits> {
    /// Build [`GrpcClient`]
    pub fn build(self) -> Result<GrpcClient<Channel>, GrpcClientBuilderError> {
        let mut tls_config = ClientTlsConfig::new();

        #[cfg(feature = "tls-native-roots")]
        if self.transport_setup.tls {
            tls_config = tls_config.with_native_roots();
        }

        #[cfg(feature = "tls-webpki-roots")]
        if self.transport_setup.tls {
            tls_config = tls_config.with_webpki_roots();
        }

        #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
        let channel = Endpoint::from_shared(self.transport_setup.url)?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        #[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
        let channel = Endpoint::from_shared(self.transport_setup.url)?
            .user_agent("celestia-grpc")?
            .connect_lazy();

        Ok(GrpcClient::new(channel, self.signer_bits))
    }
}

impl<T> GrpcClientBuilder<T>
where
    T: GrpcService<TonicBody> + Service<http::Request<TonicBody>> + Clone,
    <T as Service<http::Request<TonicBody>>>::Error: StdError + Send,
    <T as GrpcService<TonicBody>>::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <<T as GrpcService<TonicBody>>::ResponseBody as http_body::Body>::Error: StdError + Send + Sync,
{
    /// Build [`GrpcClient`]
    pub fn build(self) -> Result<GrpcClient<T>, GrpcClientBuilderError> {
        Ok(GrpcClient::new(self.transport_setup, self.signer_bits))
    }
}
