use bytes::Bytes;
use http_body::Body;
use k256::ecdsa::VerifyingKey;
use signature::Keypair;
use tonic::body::BoxBody;
use tonic::client::GrpcService;
#[cfg(not(target_arch = "wasm32"))]
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use crate::grpc::StdError;
use crate::{DocSigner, GrpcClient, Result, TxClient};

#[cfg(not(target_arch = "wasm32"))]
#[derive(thiserror::Error, Debug)]
pub enum GrpcClientBuilderCertError {
    /// Error handling certificate root
    #[error(transparent)]
    WebpkiError(#[from] webpki::Error),

    /// Could not import system certificates
    #[error("Could not import platform certificates: {errors:?}")]
    RustlsNativeCertsError {
        errors: Vec<rustls_native_certs::Error>,
    },
}

/// Builder for [`GrpcClient`] and [`TxClient`]
#[derive(Clone)]
pub struct GrpcClientBuilder<T, S> {
    connection: T,
    signer: S,
    account_pubkey: Option<VerifyingKey>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GrpcClientBuilder<(Endpoint, ClientTlsConfig), ()> {
    /// Create a new client connected to the given `url` using [`Channel`] transport
    ///
    /// [`Channel`]: tonic::transport::Channel
    pub fn with_url(url: impl Into<String>) -> Result<Self, tonic::transport::Error> {
        let endpoint = Endpoint::from_shared(url.into())?.user_agent("celestia-grpc")?;
        let tls_config = ClientTlsConfig::new();

        Ok(GrpcClientBuilder {
            connection: (endpoint, tls_config),
            signer: (),
            account_pubkey: None,
        })
    }

    /// Enables the platform’s trusted certs.
    pub fn with_native_roots(self) -> Result<Self, GrpcClientBuilderCertError> {
        let rustls_native_certs::CertificateResult { certs, errors, .. } =
            rustls_native_certs::load_native_certs();

        if certs.is_empty() {
            return Err(errors.into());
        }
        let (endpoint, tls_config) = self.connection;

        let tls_config = tls_config.trust_anchors(
            certs
                .into_iter()
                .map(|c| Ok(webpki::anchor_from_trusted_cert(&c)?.to_owned()))
                .collect::<Result<Vec<_>, GrpcClientBuilderCertError>>()?,
        );

        Ok(Self {
            connection: (endpoint, tls_config),
            ..self
        })
    }

    /// Enables the webpki roots.
    #[cfg(feature = "tls-webpki-roots")]
    pub fn with_webpki_roots(self) -> Self {
        let roots = webpki_roots::TLS_SERVER_ROOTS.iter().cloned();
        let (endpoint, tls_config) = self.connection;
        Self {
            connection: (endpoint, tls_config.trust_anchors(roots)),
            ..self
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<S> GrpcClientBuilder<(Endpoint, ClientTlsConfig), S> {
    /// Connects to the endpoint, applying tls configuration
    pub fn connect(self) -> Result<GrpcClientBuilder<Channel, S>> {
        let (endpoint, tls_config) = self.connection;
        let connection = endpoint.tls_config(tls_config)?.connect_lazy();
        Ok(GrpcClientBuilder {
            connection,
            signer: self.signer,
            account_pubkey: self.account_pubkey,
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl GrpcClientBuilder<tonic_web_wasm_client::Client, ()> {
    /// Create a new client connected to the given `url` with default
    /// settings of [`tonic_web_wasm_client::Client`].
    pub fn with_grpcweb_url(url: impl Into<String>) -> Self {
        let connection = tonic_web_wasm_client::Client::new(url.into());
        GrpcClientBuilder {
            connection,
            signer: (),
            account_pubkey: None,
        }
    }
}

impl<T> GrpcClientBuilder<T, ()> {
    /// Create a gRPC client builder using provided prepared transport
    pub fn with_transport(transport: T) -> Self {
        Self {
            connection: transport,
            signer: (),
            account_pubkey: None,
        }
    }

    /// Add signer and a public key
    pub fn with_pubkey_and_signer<S>(
        self,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> GrpcClientBuilder<T, S> {
        GrpcClientBuilder {
            connection: self.connection,
            signer,
            account_pubkey: Some(account_pubkey),
        }
    }

    /// Add signer and associated public key
    pub fn with_signer_keypair<S>(self, signer: S) -> GrpcClientBuilder<T, S>
    where
        S: Keypair<VerifyingKey = VerifyingKey>,
    {
        let account_pubkey = Some(signer.verifying_key());
        GrpcClientBuilder {
            connection: self.connection,
            signer,
            account_pubkey,
        }
    }
}

impl<T> GrpcClientBuilder<T, ()>
where
    T: GrpcService<BoxBody> + Clone,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
{
    /// Build [`GrpcClient`]
    pub fn build_client(self) -> GrpcClient<T> {
        GrpcClient::new(self.connection)
    }
}

impl<T, S> GrpcClientBuilder<T, S>
where
    //B: ToGrpcService<T>,
    T: GrpcService<BoxBody> + Clone,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    S: DocSigner,
{
    /// Build [`TxClient`]
    pub async fn build_tx_client(self) -> Result<TxClient<T, S>> {
        TxClient::new(
            self.connection,
            self.account_pubkey.expect("key to be present"),
            self.signer,
        )
        .await
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<Vec<rustls_native_certs::Error>> for GrpcClientBuilderCertError {
    fn from(errors: Vec<rustls_native_certs::Error>) -> Self {
        GrpcClientBuilderCertError::RustlsNativeCertsError { errors }
    }
}
