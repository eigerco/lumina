use std::sync::Arc;

use celestia_grpc::{DocSigner, IntoAny, TxClient, TxConfig, TxInfo};
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

pub use crate::blob::BlobApi;
pub use crate::blobstream::BlobstreamApi;
pub use crate::fraud::FraudApi;
pub use crate::header::HeaderApi;
pub use crate::share::ShareApi;
pub use crate::state::StateApi;

pub(crate) struct Context<S> {
    pub(crate) rpc: RpcClient,
    grpc: Option<TxClient<tonic::transport::Channel, S>>,
}

impl<S> Context<S>
where
    S: DocSigner,
{
    pub(crate) fn grpc(&self) -> Result<&TxClient<tonic::transport::Channel, S>> {
        self.grpc.as_ref().ok_or(Error::ReadOnlyMode)
    }
}

/// A high-level client for interacting with a Celestia node.
///
/// It combines the functionality of `celestia-rpc` and `celestia-grpc` crates.
pub struct Client<S> {
    state: StateApi<S>,
    blob: BlobApi<S>,
    header: HeaderApi<S>,
    share: ShareApi<S>,
    fraud: FraudApi<S>,
    blobstream: BlobstreamApi<S>,
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

impl<S> Client<S>
where
    S: DocSigner,
{
    /// Create a new client.
    pub async fn new(
        rpc_url: &str,
        rpc_auth_token: Option<&str>,
        grpc_url: &str,
        address: &Address,
        pubkey: VerifyingKey,
        signer: S,
    ) -> Result<Self> {
        let rpc = RpcClient::new(rpc_url, rpc_auth_token).await?;
        let grpc = TxClient::with_url(grpc_url, address, pubkey, signer).await?;

        let ctx = Arc::new(Context {
            rpc,
            grpc: Some(grpc),
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

    pub fn state(&self) -> &StateApi<S> {
        &self.state
    }

    pub fn blob(&self) -> &BlobApi<S> {
        &self.blob
    }

    pub fn header(&self) -> &HeaderApi<S> {
        &self.header
    }

    pub fn share(&self) -> &ShareApi<S> {
        &self.share
    }

    pub fn fraud(&self) -> &FraudApi<S> {
        &self.fraud
    }

    /*
    async fn get_auth_params(&self) -> Result<AuthParams>;

    /// Get account
    #[grpc_method(AuthQueryClient::account)]
    async fn get_account(&self, account: &Address) -> Result<Account>;

    /// Get accounts
    #[grpc_method(AuthQueryClient::accounts)]
    async fn get_accounts(&self) -> Result<Vec<Account>>;

    // cosmos.bank

    /// Get balance of coins with given denom
    #[grpc_method(BankQueryClient::balance)]
    async fn get_balance(&self, address: &Address, denom: impl Into<String>) -> Result<Coin>;

    /// Get balance of all coins
    #[grpc_method(BankQueryClient::all_balances)]
    async fn get_all_balances(&self, address: &Address) -> Result<Vec<Coin>>;

    /// Get balance of all spendable coins
    #[grpc_method(BankQueryClient::spendable_balances)]
    async fn get_spendable_balances(&self, address: &Address) -> Result<Vec<Coin>>;

    /// Get total supply
    #[grpc_method(BankQueryClient::total_supply)]
    async fn get_total_supply(&self) -> Result<Vec<Coin>>;

    // cosmos.base.node

    /// Get Minimum Gas price
    #[grpc_method(ConfigServiceClient::config)]
    async fn get_min_gas_price(&self) -> Result<f64>;

    // cosmos.base.tendermint

    /// Get latest block
    #[grpc_method(TendermintServiceClient::get_latest_block)]
    async fn get_latest_block(&self) -> Result<Block>;

    /// Get block by height
    #[grpc_method(TendermintServiceClient::get_block_by_height)]
    async fn get_block_by_height(&self, height: i64) -> Result<Block>;

    // cosmos.tx

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::broadcast_tx)]
    async fn broadcast_tx(&self, tx_bytes: Vec<u8>, mode: BroadcastMode) -> Result<TxResponse>;

    /// Get Tx
    #[grpc_method(TxServiceClient::get_tx)]
    async fn get_tx(&self, hash: Hash) -> Result<GetTxResponse>;

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::simulate)]
    async fn simulate(&self, tx_bytes: Vec<u8>) -> Result<GasInfo>;

    // celestia.blob

    /// Get blob params
    #[grpc_method(BlobQueryClient::params)]
    async fn get_blob_params(&self) -> Result<BlobParams>;

    // celestia.core.tx

    /// Get status of the transaction
    #[grpc_method(TxStatusClient::tx_status)]
    async fn tx_status(&self, hash: Hash) -> Result<TxStatusResponse>;

    ////////

    /// Retrieves the blob by commitment under the given namespace and height.
    pub async fn get_blob(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Blob> {
        Ok(self.rpc.blob_get(height, namespace, commitment).await?)
    }

    /// Retrieves all blobs under the given namespaces and height.
    pub async fn get_all_blobs(
        &self,
        height: u64,
        namespaces: &[Namespace],
    ) -> Result<Option<Vec<Blob>>> {
        Ok(self.rpc.blob_get_all(height, namespaces).await?)
    }

    /// Retrieves proofs in the given namespaces at the given height by commitment.
    pub async fn get_proof(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Vec<NamespaceProof>> {
        Ok(self
            .rpc
            .blob_get_proof(height, namespace, commitment)
            .await?)
    }

    /// Checks whether a blob's given commitment is included at given height and under the namespace.
    pub async fn blob_included(
        &self,
        height: u64,
        namespace: Namespace,
        proof: &NamespaceProof,
        commitment: Commitment,
    ) -> Result<bool> {
        Ok(self
            .rpc
            .blob_included(height, namespace, proof, commitment)
            .await?)
    }

    /// Subscribe to published blobs from the given namespace as they are included.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    pub async fn blob_subscribe(
        &self,
        namespace: Namespace,
    ) -> Result<Subscription<BlobsAtHeight>> {
        Ok(self.rpc.blob_subscribe(namespace).await?)
    }

    /*
     *
    Header     headerapi.Module
    State      stateapi.Module
    Share      shareapi.Module
    Fraud      fraudapi.Module
    Blobstream blobstreamapi.Module
    */


    */
}
