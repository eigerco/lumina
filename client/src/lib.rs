use std::sync::Arc;

use celestia_grpc::TxClient;
use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::{
    BlobClient, Client as RpcClient, DasClient, HeaderClient, ShareClient, StateClient,
};
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::state::Address;
use celestia_types::AppVersion;
use celestia_types::Commitment;
use jsonrpsee_core::client::Subscription;
use tendermint::chain::Id;
use tendermint::crypto::default::ecdsa_secp256k1::VerifyingKey;

mod blob;
mod blobstream;
mod fraud;
mod header;
mod share;
mod state;
pub(crate) mod utils;

pub use crate::blob::BlobApi;
pub use crate::blobstream::BlobstreamApi;
pub use crate::fraud::FraudApi;
pub use crate::header::HeaderApi;
pub use crate::share::ShareApi;
pub use crate::state::StateApi;

use crate::utils::DispatchedDocSigner;

#[doc(inline)]
pub use celestia_grpc::{DocSigner, IntoAny, TxConfig, TxInfo};

pub(crate) struct Context {
    pub(crate) rpc: RpcClient,
    grpc: Option<TxClient<tonic::transport::Channel, DispatchedDocSigner>>,
}

impl Context {
    pub(crate) fn grpc(&self) -> Result<&TxClient<tonic::transport::Channel, DispatchedDocSigner>> {
        self.grpc.as_ref().ok_or(Error::ReadOnlyMode)
    }
}

/// A high-level client for interacting with a Celestia node.
///
/// It combines the functionality of `celestia-rpc` and `celestia-grpc` crates.
pub struct Client {
    ctx: Arc<Context>,
    state: StateApi,
    blob: BlobApi,
    header: HeaderApi,
    share: ShareApi,
    fraud: FraudApi,
    blobstream: BlobstreamApi,
}

/// Representation of all the errors that can occur when interacting with [`celestia_client`].
///
/// [`celestia_client`]: crate
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// [`celestia_rpc::Error`]
    #[error("RPC error: {0}")]
    Rpc(#[from] celestia_rpc::Error),

    /// [`celestia_grpc::Error`]
    #[error("GRPC error: {0}")]
    Grpc(#[from] celestia_grpc::Error),

    /// Client is in read-only mode
    #[error("Client is constructed for read-only mode")]
    ReadOnlyMode,

    /// Invalid height
    #[error("Invalid height: {0}")]
    InvalidHeight(u64),
}

impl From<jsonrpsee_core::client::error::Error> for Error {
    fn from(value: jsonrpsee_core::client::error::Error) -> Self {
        Error::Rpc(celestia_rpc::Error::JsonRpc(value))
    }
}

/// Alias for a `Result` with the error type [`celestia_client::Error`].
///
/// [`celestia_client::Error`]: crate::Error
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Client {
    /// Create a new client.
    pub async fn new<S>(
        rpc_url: &str,
        rpc_auth_token: Option<&str>,
        grpc_url: &str,
        address: &Address,
        pubkey: VerifyingKey,
        signer: S,
    ) -> Result<Self>
    where
        S: DocSigner + 'static,
    {
        let signer = DispatchedDocSigner::new(signer);
        let rpc = RpcClient::new(rpc_url, rpc_auth_token).await?;
        let grpc = TxClient::with_url(grpc_url, address, pubkey, signer).await?;

        let ctx = Arc::new(Context {
            rpc,
            grpc: Some(grpc),
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

    pub async fn submit_message<M>(&self, message: M, cfg: TxConfig) -> Result<TxInfo>
    where
        M: IntoAny,
    {
        Ok(self.ctx.grpc()?.submit_message(message, cfg).await?)
    }

    pub fn last_seen_gas_price(&self) -> Result<f64> {
        Ok(self.ctx.grpc()?.last_seen_gas_price())
    }

    pub fn chain_id(&self) -> Result<Id> {
        Ok(self.ctx.grpc()?.chain_id().to_owned())
    }

    pub fn app_version(&self) -> Result<AppVersion> {
        Ok(self.ctx.grpc()?.app_version())
    }

    pub fn state(&self) -> &StateApi {
        &self.state
    }

    pub fn blob(&self) -> &BlobApi {
        &self.blob
    }

    pub fn header(&self) -> &HeaderApi {
        &self.header
    }

    pub fn share(&self) -> &ShareApi {
        &self.share
    }

    pub fn fraud(&self) -> &FraudApi {
        &self.fraud
    }
}
