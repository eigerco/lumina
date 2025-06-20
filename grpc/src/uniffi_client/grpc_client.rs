//! GRPC client wrapper for uniffi

use tonic::transport::Channel;
use uniffi::Object;

use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::hash::uniffi_types::UniffiHash;
use celestia_types::state::auth::{Account, AuthParams};
use celestia_types::state::{Address, Coin, TxResponse};
use celestia_types::UniffiConversionError;

use crate::grpc::{BroadcastMode, GasInfo, GetTxResponse, TxStatusResponse};

/// Alias for a `Result` with the error type [`GrpcClientError`]
pub type Result<T, E = GrpcClientError> = std::result::Result<T, E>;

/// Errors returned from [`GrpcClient`]
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcClientError {
    /// Error from grpc client
    #[error("grpc error: {msg}")]
    GrpcError {
        /// error message
        msg: String,
    },

    /// Error during uniffi types conversion
    #[error("uniffi conversion error: {msg}")]
    UniffiConversionError {
        /// error message
        msg: String,
    },
}

type InnerClient = crate::GrpcClient<Channel>;

/// Celestia GRPC client
#[derive(Object)]
pub struct GrpcClient {
    client: InnerClient,
}

#[uniffi::export(async_runtime = "tokio")]
impl GrpcClient {
    /// Create a new client connected with the given `url`
    #[uniffi::constructor]
    // this function _must_ be async despite not awaiting, so that it executes in tokio runtime
    // context
    pub async fn new(url: String) -> Result<Self> {
        Ok(GrpcClient {
            client: InnerClient::with_url(url).map_err(crate::Error::TransportError)?,
        })
    }

    /// Get auth params
    pub async fn get_auth_params(&self) -> Result<AuthParams> {
        Ok(self.client.get_auth_params().await?)
    }

    /// Get account
    pub async fn get_account(&self, account: &Address) -> Result<Account> {
        Ok(self.client.get_account(account).await?)
    }

    /// Get accounts
    pub async fn get_accounts(&self) -> Result<Vec<Account>> {
        Ok(self.client.get_accounts().await?)
    }

    /// Get balance of coins with given denom
    pub async fn get_balance(&self, address: &Address, denom: String) -> Result<Coin> {
        Ok(self.client.get_balance(address, denom).await?)
    }

    /// Get balance of all coins
    pub async fn get_all_balances(&self, address: &Address) -> Result<Vec<Coin>> {
        Ok(self.client.get_all_balances(address).await?)
    }

    /// Get balance of all spendable coins
    pub async fn get_spendable_balances(&self, address: &Address) -> Result<Vec<Coin>> {
        Ok(self.client.get_spendable_balances(address).await?)
    }

    /// Get total supply
    pub async fn get_total_supply(&self) -> Result<Vec<Coin>> {
        Ok(self.client.get_total_supply().await?)
    }

    /// Get Minimum Gas price
    async fn get_min_gas_price(&self) -> Result<f64> {
        Ok(self.client.get_min_gas_price().await?)
    }

    /// Get latest block
    async fn get_latest_block(&self) -> Result<Block> {
        Ok(self.client.get_latest_block().await?)
    }

    /// Get block by height
    async fn get_block_by_height(&self, height: i64) -> Result<Block> {
        Ok(self.client.get_block_by_height(height).await?)
    }

    /// Broadcast prepared and serialised transaction
    async fn broadcast_tx(&self, tx_bytes: Vec<u8>, mode: BroadcastMode) -> Result<TxResponse> {
        Ok(self.client.broadcast_tx(tx_bytes, mode).await?)
    }

    /// Get Tx
    async fn get_tx(&self, hash: UniffiHash) -> Result<GetTxResponse> {
        Ok(self.client.get_tx(hash.try_into()?).await?)
    }

    /// Broadcast prepared and serialised transaction
    async fn simulate(&self, tx_bytes: Vec<u8>) -> Result<GasInfo> {
        Ok(self.client.simulate(tx_bytes).await?)
    }

    /// Get blob params
    async fn get_blob_params(&self) -> Result<BlobParams> {
        Ok(self.client.get_blob_params().await?)
    }

    /// Get status of the transaction
    async fn tx_status(&self, hash: UniffiHash) -> Result<TxStatusResponse> {
        Ok(self.client.tx_status(hash.try_into()?).await?)
    }
}

impl From<crate::Error> for GrpcClientError {
    fn from(value: crate::Error) -> Self {
        GrpcClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}

impl From<UniffiConversionError> for GrpcClientError {
    fn from(value: UniffiConversionError) -> Self {
        GrpcClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}
