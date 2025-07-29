use std::fmt::{self, Debug};
use std::sync::Arc;

use celestia_grpc::TxClient;
use celestia_rpc::{Client as RpcClient, HeaderClient};
use zeroize::Zeroizing;

use crate::blob::BlobApi;
use crate::blobstream::BlobstreamApi;
use crate::fraud::FraudApi;
use crate::header::HeaderApi;
use crate::share::ShareApi;
use crate::state::StateApi;
use crate::tx::{DocSigner, Keypair, SigningKey, VerifyingKey};
use crate::types::state::AccAddress;
use crate::types::ExtendedHeader;
use crate::utils::DispatchedDocSigner;
use crate::{Error, Result};

#[cfg(not(target_arch = "wasm32"))]
type Transport = tonic::transport::Channel;

#[cfg(target_arch = "wasm32")]
type Transport = tonic_web_wasm_client::Client;

pub(crate) struct Context {
    pub(crate) rpc: RpcClient,
    grpc: Option<TxClient<Transport, DispatchedDocSigner>>,
    pubkey: Option<VerifyingKey>,
    chain_id: tendermint::chain::Id,
}

/// A high-level client for interacting with a Celestia node.
///
/// There are two modes: read-only mode and submit mode. Read-only mode requires
/// RPC endpoint, while submit mode requires RPC and gRPC endpoints plus a signer.
///
/// # Examples
///
/// Read-only mode:
///
/// ```no_run
/// # use celestia_client::{Client, Result};
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rpc_url("ws://localhost:26658")
///     .build()
///     .await?;
///
/// client.header().head().await?;
/// # Ok(())
/// # }
/// ```
///
/// Submit mode:
///
/// ```no_run
/// # use celestia_client::{Client, Result};
/// # use celestia_client::tx::TxConfig;
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rpc_url("ws://localhost:26658")
///     .grpc_url("http://localhost:9090")
///     .private_key_hex("393fdb5def075819de55756b45c9e2c8531a8c78dd6eede483d3440e9457d839")
///     .build()
///     .await?;
///
/// let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
/// client.state().transfer(&to_address, 12345, TxConfig::default()).await?;
/// # Ok(())
/// # }
/// ```
///
/// [`celestia-rpc`]: celestia_rpc
/// [`celestia-grpc`]: celestia_grpc
pub struct Client {
    ctx: Arc<Context>,
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
    signer: Option<SignerKind>,
}

enum SignerKind {
    Signer((VerifyingKey, DispatchedDocSigner)),
    PrivKeyBytes(Zeroizing<Vec<u8>>),
    PrivKeyHex(Zeroizing<String>),
}

impl Debug for SignerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SignerKind::Signer(..) => "SignerKind::Signer(..)",
            SignerKind::PrivKeyBytes(..) => "SignerKind::PrivKeyBytes(..)",
            SignerKind::PrivKeyHex(..) => "SignerKind::PrivKeyHex(..)",
        };
        f.write_str(s)
    }
}

impl Context {
    pub(crate) fn grpc(&self) -> Result<&TxClient<Transport, DispatchedDocSigner>> {
        self.grpc.as_ref().ok_or(Error::ReadOnlyMode)
    }

    pub(crate) fn pubkey(&self) -> Result<&VerifyingKey> {
        self.pubkey.as_ref().ok_or(Error::ReadOnlyMode)
    }

    pub(crate) fn address(&self) -> Result<AccAddress> {
        let pubkey = self.pubkey()?.to_owned();
        Ok(AccAddress::new(pubkey.into()))
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

    /// Returns chain id of the network.
    pub fn chain_id(&self) -> &tendermint::chain::Id {
        &self.ctx.chain_id
    }

    /// Returns the public key of the signer.
    pub fn pubkey(&self) -> Result<VerifyingKey> {
        self.ctx.pubkey().cloned()
    }

    /// Returns the address of signer.
    pub fn address(&self) -> Result<AccAddress> {
        self.ctx.address()
    }

    /// Returns state API accessor.
    pub fn state(&self) -> &StateApi {
        &self.state
    }

    /// Returns blob API accessor.
    pub fn blob(&self) -> &BlobApi {
        &self.blob
    }

    /// Returns blobstream API accessor.
    pub fn blobstream(&self) -> &BlobstreamApi {
        &self.blobstream
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
    pub fn signer<S>(mut self, pubkey: VerifyingKey, signer: S) -> ClientBuilder
    where
        S: DocSigner + Sync + Send + 'static,
    {
        let signer = DispatchedDocSigner::new(signer);
        self.signer = Some(SignerKind::Signer((pubkey, signer)));
        self
    }

    /// Set signer from a keypair.
    pub fn keypair<S>(self, keypair: S) -> ClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + Sync + Send + 'static,
    {
        let pubkey = keypair.verifying_key();
        self.signer(pubkey, keypair)
    }

    /// Set signer from a raw private key.
    pub fn private_key(mut self, bytes: &[u8]) -> ClientBuilder {
        let bytes = Zeroizing::new(bytes.to_vec());
        self.signer = Some(SignerKind::PrivKeyBytes(bytes));
        self
    }

    /// Set signer from a hex formatted private key.
    pub fn private_key_hex(mut self, s: &str) -> ClientBuilder {
        let s = Zeroizing::new(s.to_string());
        self.signer = Some(SignerKind::PrivKeyHex(s));
        self
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

    /// Set the gRPC endpoint.
    ///
    /// # Note
    ///
    /// In WASM the endpoint needs to support gRPC-Web.
    pub fn grpc_url(mut self, url: &str) -> ClientBuilder {
        self.grpc_url = Some(url.to_owned());
        self
    }

    /// Build [`Client`].
    pub async fn build(mut self) -> Result<Client> {
        let rpc_url = self.rpc_url.as_ref().ok_or(Error::RpcEndpointNotSet)?;
        let rpc_auth_token = self.rpc_auth_token.as_deref();

        #[cfg(target_arch = "wasm32")]
        if rpc_auth_token.is_some() {
            return Err(Error::AuthTokenNotSupported);
        }

        let signer = match self.signer.take() {
            Some(SignerKind::Signer((pubkey, signer))) => Some((pubkey, signer)),
            Some(SignerKind::PrivKeyBytes(bytes)) => Some(priv_key_signer(&bytes)?),
            Some(SignerKind::PrivKeyHex(s)) => {
                let bytes =
                    Zeroizing::new(hex::decode(s.trim()).map_err(|_| Error::InvalidPrivateKey)?);
                Some(priv_key_signer(&bytes)?)
            }
            None => None,
        };

        let (pubkey, grpc) = match (&self.grpc_url, signer) {
            (Some(url), Some((pubkey, signer))) => {
                #[cfg(not(target_arch = "wasm32"))]
                let tx_client = TxClient::with_url(url, pubkey, signer).await?;

                #[cfg(target_arch = "wasm32")]
                let tx_client = TxClient::with_grpcweb_url(url, pubkey, signer).await?;

                (Some(pubkey), Some(tx_client))
            }
            (Some(_url), None) => return Err(Error::SignerNotSet),
            (None, Some((_pubkey, _signer))) => return Err(Error::GrpcEndpointNotSet),
            (None, None) => (None, None),
        };

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = RpcClient::new(rpc_url, rpc_auth_token).await?;

        #[cfg(target_arch = "wasm32")]
        let rpc = RpcClient::new(rpc_url).await?;

        let head = rpc.header_network_head().await?;
        head.validate()?;

        if let Some(grpc) = &grpc {
            if grpc.chain_id() != head.chain_id() {
                return Err(Error::ChainIdMissmatch);
            }
        }

        let ctx = Arc::new(Context {
            rpc,
            grpc,
            pubkey,
            chain_id: head.chain_id().to_owned(),
        });

        Ok(Client {
            ctx: ctx.clone(),
            blob: BlobApi::new(ctx.clone()),
            header: HeaderApi::new(ctx.clone()),
            share: ShareApi::new(ctx.clone()),
            fraud: FraudApi::new(ctx.clone()),
            blobstream: BlobstreamApi::new(ctx.clone()),
            state: StateApi::new(ctx.clone()),
        })
    }
}

fn priv_key_signer(bytes: &[u8]) -> Result<(VerifyingKey, DispatchedDocSigner)> {
    let signing_key = SigningKey::from_slice(bytes).map_err(|_| Error::InvalidPrivateKey)?;
    let pubkey = signing_key.verifying_key().to_owned();
    let signer = DispatchedDocSigner::new(signing_key);
    Ok((pubkey, signer))
}
