use std::sync::Arc;

use celestia_grpc::TxClient;
use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::{
    BlobClient, Client as RpcClient, DasClient, HeaderClient, ShareClient, StateClient,
};
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::state::Address;
use celestia_types::Commitment;
use celestia_types::{AppVersion, ExtendedHeader};
use k256::ecdsa::signature::Keypair;
use k256::ecdsa::SigningKey;
use tendermint::chain::Id;
use tendermint::crypto::default::ecdsa_secp256k1::VerifyingKey;
use zeroize::Zeroizing;

use crate::blob::BlobApi;
use crate::blobstream::BlobstreamApi;
use crate::fraud::FraudApi;
use crate::header::HeaderApi;
use crate::share::ShareApi;
use crate::state::StateApi;
use crate::tx::DocSigner;
use crate::utils::DispatchedDocSigner;
use crate::{Error, Result};

pub(crate) struct Context {
    pub(crate) rpc: RpcClient,
    grpc: Option<TxClient<tonic::transport::Channel, DispatchedDocSigner>>,
    pubkey: Option<VerifyingKey>,
}

/// A high-level client for interacting with a Celestia node.
///
/// There are two modes: read-only mode and submit mode. Read-only mode requires
/// RPC endpoint and submit mode requires RPC/GRPC endpoints, and a signer.
///
/// # Examples
///
/// Read-only mode:
///
/// ```no_run
/// # use celestia_client::Result;
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rcp_url(RPC_URL)
///     .await?;
///
/// client.header().head().await?;
/// # }
/// ```
///
/// Submit mode:
///
/// ```no_run
/// # use celestia_client::Result;
/// # use k256::ecdsa::SigningKey;
/// # const RPC_URL: &str = "http://localhost:26658";
/// # const GRPC_URL : &str = "http://localhost:19090";
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rcp_url(RPC_URL)
///     .grpc_url(GRPC_URL)
///     .plaintext_private_key("...")
///     .await?;
///
/// let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
/// client.state().transfer(to_address, 12345, TxConfig::default()).await?;
/// # }
/// ```
///
/// [`celestia-rpc`]: celestia_rpc
/// [`celestia-grpc`]: celestia_grpc
pub struct Client {
    state: StateApi,
    blob: BlobApi,
    header: HeaderApi,
    share: ShareApi,
    fraud: FraudApi,
    blobstream: BlobstreamApi,
}

/// A builder for [`Client`].
#[derive(Debug, Default)]
pub struct ClientBuilder {
    rpc_url: Option<String>,
    rpc_auth_token: Option<String>,
    grpc_url: Option<String>,
    pubkey: Option<VerifyingKey>,
    signer: Option<DispatchedDocSigner>,
}

impl Context {
    pub(crate) fn grpc(&self) -> Result<&TxClient<tonic::transport::Channel, DispatchedDocSigner>> {
        self.grpc.as_ref().ok_or(Error::ReadOnlyMode)
    }

    pub(crate) fn pubkey(&self) -> Result<&VerifyingKey> {
        self.pubkey.as_ref().ok_or(Error::ReadOnlyMode)
    }

    pub(crate) async fn get_header_validated(&self, height: u64) -> Result<ExtendedHeader> {
        let header = self.rpc.header_get_by_height(height).await?;
        header.validate()?;
        Ok(header)
    }
}

impl Client {
    /// Returns `ClientBuilder`.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Returns state API accessor.
    pub fn state(&self) -> &StateApi {
        &self.state
    }

    /// Returns blob API accessor.
    pub fn blob(&self) -> &BlobApi {
        &self.blob
    }

    /// Returns header API accessor.
    pub fn header(&self) -> &HeaderApi {
        &self.header
    }

    /// Returns share API accessor.
    pub fn share(&self) -> &ShareApi {
        &self.share
    }

    /// Returns fraud API accessor.
    pub fn fraud(&self) -> &FraudApi {
        &self.fraud
    }
}

impl ClientBuilder {
    /// Returns a new builder.
    pub fn new() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Set signer and its public key.
    pub fn singer<S>(mut self, pubkey: VerifyingKey, signer: S) -> ClientBuilder
    where
        S: DocSigner + 'static,
    {
        self.pubkey = Some(pubkey);
        self.signer = Some(DispatchedDocSigner::new(signer));
        self
    }

    /// Set signer from a keypair.
    pub fn keypair<S>(mut self, keypair: S) -> ClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = keypair.verifying_key();
        self.singer(pubkey, keypair)
    }

    /// Set signer from a plaintext private key.
    pub fn plaintext_private_key(mut self, s: &str) -> Result<ClientBuilder> {
        let bytes = Zeroizing::new(hex::decode(s).map_err(|_| Error::InvalidPrivateKey)?);
        let signing_key = SigningKey::from_slice(&*bytes).map_err(|_| Error::InvalidPrivateKey)?;
        Ok(self.keypair(signing_key))
    }

    /// Set the RPC endpoint.
    pub fn rpc_url(mut self, url: &str) -> ClientBuilder {
        self.rpc_url = Some(url.to_owned());
        self
    }

    /// Set the authentication token of RPC endpoint.
    pub fn rpc_auth_token(mut self, auth_token: &str) -> ClientBuilder {
        self.rpc_auth_token = Some(auth_token.to_owned());
        self
    }

    /// Set the GRPC endpoint.
    pub fn grpc_url(mut self, url: &str) -> ClientBuilder {
        self.grpc_url = Some(url.to_owned());
        self
    }

    /// Build [`Client`].
    pub async fn build(mut self) -> Result<Client> {
        let rpc_url = self.rpc_url.as_ref().ok_or(Error::RpcEndpointNotSet)?;
        let rpc_auth_token = self.rpc_auth_token.as_deref();

        let grpc = match (&self.grpc_url, &self.pubkey, self.signer.take()) {
            (Some(url), Some(pubkey), Some(signer)) => {
                Some(TxClient::with_url(url, pubkey.to_owned(), signer).await?)
            }
            (Some(url), None, None) => return Err(Error::SignerNotSet),
            (None, Some(pubkey), Some(signer)) => return Err(Error::GrpcEndpointNotSet),
            (None, None, None) => None,
            _ => unreachable!(),
        };

        let rpc = RpcClient::new(rpc_url, rpc_auth_token).await?;

        let ctx = Arc::new(Context {
            rpc,
            grpc,
            pubkey: self.pubkey.clone(),
        });

        Ok(Client {
            blob: BlobApi::new(ctx.clone()),
            header: HeaderApi::new(ctx.clone()),
            share: ShareApi::new(ctx.clone()),
            fraud: FraudApi::new(ctx.clone()),
            blobstream: BlobstreamApi::new(ctx.clone()),
            state: StateApi::new(ctx.clone()),
        })
    }
}
