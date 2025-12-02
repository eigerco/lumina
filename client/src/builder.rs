use std::error::Error as StdError;
use std::time::Duration;

use blockstore::cond_send::CondSend;
use celestia_grpc::GrpcClientBuilder;
use celestia_rpc::{ClientBuilder as RpcClientBuilder, HeaderClient};
use derive_builder::Builder;
use http::Request;
use tonic::body::Body as TonicBody;
use tonic::codegen::{Bytes, Service};
use tonic::metadata::MetadataMap;

use crate::Client;
use crate::client::ClientInner;
use crate::tx::{DocSigner, Keypair, VerifyingKey};
use crate::{Error, Result};

///
/// A builder for [`Client`].
#[derive(Debug, Default, Builder)]
#[builder(name = "ClientBuilder", pattern = "owned")]
#[builder(build_fn(private, name = "build_partial"))]
pub struct ClientPartialBuilder {
    /// Set the request timeout for both RPC and gRPC endpoints.
    #[builder(setter(strip_option))]
    timeout: Option<Duration>,
    #[builder(setter(custom), field(ty = "RpcClientBuilder"))]
    rpc_builder: RpcClientBuilder,
    #[builder(setter(custom), field(ty = "Option<GrpcClientBuilder>"))]
    grpc_builder: Option<GrpcClientBuilder>,
}

impl ClientBuilder {
    /// Returns a new builder.
    pub fn new() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Set signer and its public key.
    pub fn signer<S>(mut self, pubkey: VerifyingKey, signer: S) -> ClientBuilder
    where
        S: DocSigner + Sync + Send + 'static,
    {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.pubkey_and_signer(pubkey, signer));
        self
    }

    /// Set signer from a keypair.
    pub fn keypair<S>(mut self, keypair: S) -> ClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + Sync + Send + 'static,
    {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.signer_keypair(keypair));
        self
    }

    /// Set signer from a raw private key.
    pub fn private_key(mut self, bytes: &[u8]) -> ClientBuilder {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.private_key(bytes));
        self
    }

    /// Set signer from a hex formatted private key.
    pub fn private_key_hex(mut self, s: &str) -> ClientBuilder {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.private_key_hex(s));
        self
    }

    /// Set the RPC endpoint.
    pub fn rpc_url(mut self, url: &str) -> ClientBuilder {
        self.rpc_builder = self.rpc_builder.url(url.to_string());
        self
    }

    /// Set the authentication token of RPC endpoint.
    pub fn rpc_auth_token(mut self, auth_token: impl Into<String>) -> ClientBuilder {
        self.rpc_builder = self.rpc_builder.auth_token(auth_token.into());
        self
    }

    /// Set the gRPC endpoint.
    ///
    /// # Note
    ///
    /// In WASM the endpoint needs to support gRPC-Web.
    pub fn grpc_url(mut self, url: &str) -> ClientBuilder {
        //let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(self.grpc_builder.unwrap_or_default().url(url));
        self
    }

    /// Set manually configured gRPC transport
    pub fn grpc_transport<B, T>(mut self, transport: T) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
        <B as http_body::Body>::Error: StdError + Send + Sync,
        T: Service<Request<TonicBody>, Response = http::Response<B>>
            + Send
            + Sync
            + Clone
            + 'static,
        <T as Service<Request<TonicBody>>>::Error: StdError + Send + Sync + 'static,
        <T as Service<Request<TonicBody>>>::Future: CondSend + 'static,
    {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.transport(transport));
        self
    }

    /// Appends ascii metadata to all requests made by the client.
    pub fn grpc_metadata(mut self, key: &str, value: &str) -> Self {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.metadata(key, value));
        self
    }

    /// Appends binary metadata to all requests made by the client.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    pub fn grpc_metadata_bin(mut self, key: &str, value: &[u8]) -> Self {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.metadata_bin(key, value));
        self
    }

    /// Sets the initial metadata map that will be attached to all requestes made by the client.
    pub fn grpc_metadata_map(mut self, metadata: MetadataMap) -> Self {
        let grpc_builder = self.grpc_builder.unwrap_or_default();
        self.grpc_builder = Some(grpc_builder.metadata_map(metadata));
        self
    }

    /// Build [`Client`].
    pub async fn build(self) -> Result<Client> {
        let ClientPartialBuilder {
            timeout,
            rpc_builder,
            grpc_builder,
        } = self
            .build_partial()
            .expect("derive_builder checks should allways succeed here");

        let grpc = grpc_builder
            .map(|b| b.maybe_timeout(timeout).build())
            .transpose()?;

        let pubkey = grpc.as_ref().and_then(|c| c.get_account_pubkey());

        let rpc = rpc_builder
            .maybe_connect_timeout(timeout)
            .maybe_request_timeout(timeout)
            .build()
            .await?;

        let head = rpc.header_network_head().await?;
        head.validate()?;

        if let Some(grpc) = &grpc {
            if &grpc.chain_id().await? != head.chain_id() {
                return Err(Error::ChainIdMissmatch);
            }
        }

        let inner = ClientInner {
            rpc,
            grpc,
            pubkey,
            chain_id: head.chain_id().to_owned(),
        };

        Ok(inner.into())
    }
}
