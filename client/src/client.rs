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
use tendermint::chain::Id;
use tendermint::crypto::default::ecdsa_secp256k1::VerifyingKey;

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

pub struct ClientBuilder {
    rpc_url: String,
    rpc_auth_token: Option<String>,
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
        let grpc = TxClient::with_url(grpc_url, address, pubkey.clone(), signer).await?;

        let ctx = Arc::new(Context {
            rpc,
            grpc: Some(grpc),
            pubkey: Some(pubkey),
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
