use bytes::Bytes;
use http_body::Body;
use k256::ecdsa::VerifyingKey;
use signature::Keypair;
use tonic::body::BoxBody;
use tonic::client::GrpcService;
#[cfg(not(target_arch = "wasm32"))]
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use crate::client::SignerBits;
use crate::client::StdError;
use crate::signer::FullSigner;
use crate::GrpcClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::GrpcClientBuilderError;

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeTransportBits {
    url: String,
    load_native_roots: bool,
    load_webpki_roots: bool,
}

/// Builder for [`GrpcClient`] and [`TxClient`]
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
                load_native_roots: false,
                load_webpki_roots: false,
            },
            signer_bits: None,
        }
    }

    /// Enables the platform’s trusted certs.
    #[cfg(feature = "tls-native-roots")]
    pub fn with_native_roots(self) -> Self {
        let NativeTransportBits {
            url,
            load_webpki_roots,
            ..
        } = self.transport_setup;

        Self {
            transport_setup: NativeTransportBits {
                url,
                load_native_roots: true,
                load_webpki_roots,
            },
            ..self
        }
    }

    /// Enables the webpki roots.
    #[cfg(feature = "tls-webpki-roots")]
    pub fn with_webpki_roots(self) -> Self {
        let NativeTransportBits {
            url,
            load_native_roots,
            ..
        } = self.transport_setup;

        Self {
            transport_setup: NativeTransportBits {
                url,
                load_native_roots,
                load_webpki_roots: true,
            },
            ..self
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl GrpcClientBuilder<tonic_web_wasm_client::Client> {
    /// Create a new client connected to the given `url` with default
    /// settings of [`tonic_web_wasm_client::Client`].
    pub fn with_grpcweb_url(url: impl Into<String>) -> Self {
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
    pub fn with_pubkey_and_signer<S: FullSigner>(
        self,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> GrpcClientBuilder<T> {
        GrpcClientBuilder {
            transport_setup: self.transport_setup,
            signer_bits: Some(SignerBits {
                signer: Box::new(signer),
                pubkey: account_pubkey,
            }),
        }
    }

    /// Add signer and associated public key
    pub fn with_signer_keypair<S>(self, signer: S) -> GrpcClientBuilder<T>
    where
        S: FullSigner + Keypair<VerifyingKey = VerifyingKey>,
    {
        let pubkey = signer.verifying_key();
        GrpcClientBuilder {
            transport_setup: self.transport_setup,
            signer_bits: Some(SignerBits {
                signer: Box::new(signer),
                pubkey,
            }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl GrpcClientBuilder<NativeTransportBits> {
    pub fn build(self) -> Result<GrpcClient<Channel>, GrpcClientBuilderError> {
        let mut tls_config = ClientTlsConfig::new();

        #[cfg(feature = "tls-native-roots")]
        if self.transport_setup.load_native_roots {
            let rustls_native_certs::CertificateResult { certs, errors, .. } =
                rustls_native_certs::load_native_certs();

            if certs.is_empty() {
                return Err(errors.into());
            }

            tls_config = tls_config.trust_anchors(
                certs
                    .into_iter()
                    .map(|c| Ok(webpki::anchor_from_trusted_cert(&c)?.to_owned()))
                    .collect::<Result<Vec<_>, GrpcClientBuilderError>>()?,
            );
        }

        #[cfg(feature = "tls-webpki-roots")]
        if self.transport_setup.load_webpki_roots {
            let roots = webpki_roots::TLS_SERVER_ROOTS.iter().cloned();
            tls_config = tls_config.trust_anchors(roots);
        }

        let channel = Endpoint::from_shared(self.transport_setup.url)?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        Ok(GrpcClient::new(channel, self.signer_bits))
    }
}

impl<T> GrpcClientBuilder<T>
where
    T: GrpcService<BoxBody> + Clone,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
{
    /// Build [`GrpcClient`]
    pub fn build(self) -> GrpcClient<T> {
        GrpcClient::new(self.transport_setup, self.signer_bits)
    }
}
