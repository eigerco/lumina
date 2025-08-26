use std::error::Error as StdError;

use bytes::Bytes;
use http_body::Body;
use k256::ecdsa::VerifyingKey;
use signature::Keypair;
use tonic::body::Body as TonicBody;
use tonic::client::GrpcService;
use tonic::codegen::Service;
use tonic::transport::ClientTlsConfig;
#[cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "tls-native-roots", feature = "tls-webpki-roots")
))]
#[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
#[cfg(not(target_arch = "wasm32"))]
use tonic::transport::{Channel, Endpoint};

use crate::client::{BoxedTransport, GrpcTransport, SignerBits};
use crate::signer::DispatchedDocSigner;
use crate::{DocSigner, GrpcClient, GrpcClientBuilderError};

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeTransportBits {
    url: String,
    tls: bool,
}

enum TransportSetup {
    Configuration { 
        url:String,
        tls: bool,
    },
    BoxedTransport(BoxedTransport)
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
            transport: TransportSetup::Configuration { url: url.into(), tls: false },
            signer_bits: None,
        }
    }

    /// Create a gRPC client builder using provided prepared transport
    pub fn with_transport<T>(transport: T) -> Self 
        where T: GrpcTransport + 'static
    {
        Self {
            transport: TransportSetup::BoxedTransport(Box::new(transport)),
            signer_bits: None,
        }
    }

    /// Enables loading the certificate roots which were enabled by feature flags
    /// `tls-webpki-roots` and `tls-native-roots`.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
    pub fn with_default_tls(self) -> Result<Self, GrpcClientBuilderError> {
        let TransportSetup::Configuration { url, .. } = self.transport else {
            return Err(GrpcClientBuilderError::CannotEnableTlsOnManualTransport)
        };

        Ok(Self {
            transport: TransportSetup::Configuration { 
                url,
                tls: true,
            },
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
        /*
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
        */

        Ok(GrpcClient::new(transport, self.signer_bits))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_transport(url: String, tls: bool) -> Result<BoxedTransport, tonic::transport::Error> {
        let mut tls_config = ClientTlsConfig::new();

        #[cfg(feature = "tls-native-roots")]
        if tls {
            tls_config = tls_config.with_native_roots();
        }

        #[cfg(feature = "tls-webpki-roots")]
        if tls {
            tls_config = tls_config.with_webpki_roots();
        }

        #[cfg(any(feature = "tls-native-roots", feature = "tls-webpki-roots"))]
        let channel = Endpoint::from_shared(url)?
            .user_agent("celestia-grpc")?
            .tls_config(tls_config)?
            .connect_lazy();

        #[cfg(not(any(feature = "tls-native-roots", feature = "tls-webpki-roots")))]
        let channel = Endpoint::from_shared(url)?
            .user_agent("celestia-grpc")?
            .connect_lazy();

    //Ok(Box::new(channel))
    todo!()
}


#[cfg(target_arch = "wasm32")]
fn build_transport(url: String, tls: bool) -> Result<BoxedTransport, ()> {
    let channel = tonic_web_wasm_client::Client::new(url);

    //Ok(Box::new(channel))
        todo!()
}

/*
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
*/

/*
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
*/

/*
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
*/
