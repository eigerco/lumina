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
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
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

impl ClientBuilder {
    pub fn new() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub fn singer<S>(mut self, pubkey: VerifyingKey, signer: S) -> ClientBuilder
    where
        S: DocSigner + 'static,
    {
        self.pubkey = Some(pubkey);
        self.signer = Some(DispatchedDocSigner::new(signer));
        self
    }

    pub fn keypair<S>(mut self, keypair: S) -> ClientBuilder
    where
        S: DocSigner + Keypair<VerifyingKey = VerifyingKey> + 'static,
    {
        let pubkey = keypair.verifying_key();
        self.singer(pubkey, keypair)
    }

    pub fn bridge_node_url(mut self, url: &str) -> ClientBuilder {
        self.rpc_url = Some(url.to_owned());
        self
    }

    pub fn bridge_node_auth_token(mut self, auth_token: &str) -> ClientBuilder {
        self.rpc_auth_token = Some(auth_token.to_owned());
        self
    }

    pub fn consensus_node_url(mut self, url: &str) -> ClientBuilder {
        self.grpc_url = Some(url.to_owned());
        self
    }

    pub async fn build(mut self) -> Result<Client> {
        let rpc_url = self.rpc_url.as_ref().ok_or(Error::BridgeNodeNotSet)?;
        let rpc_auth_token = self.rpc_auth_token.as_deref();

        let grpc = match (&self.grpc_url, &self.pubkey, self.signer.take()) {
            (Some(url), Some(pubkey), Some(signer)) => {
                Some(TxClient::with_url(url, pubkey.to_owned(), signer).await?)
            }
            (Some(url), None, None) => return Err(Error::SignerNotSet),
            (None, Some(pubkey), Some(signer)) => return Err(Error::ConsensusNodeNotSet),
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
