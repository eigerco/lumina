//! GRPC client wrapper for uniffi

use tonic::transport::Channel;
use uniffi::Object;

use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::hash::uniffi_types::UniffiHash;
use celestia_types::state::auth::{Account, AuthParams};
use celestia_types::state::{AbciQueryResponse, AccAddress, Address, Coin, TxResponse};
use celestia_types::{ExtendedHeader, UniffiConversionError};

use crate::grpc::{
    BroadcastMode, GasEstimate, GasInfo, GetTxResponse, TxPriority, TxStatusResponse,
};

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
    pub async fn get_account(&self, account: &AccAddress) -> Result<Account> {
        Ok(self.client.get_account(account).await?)
    }

    /// Get accounts
    pub async fn get_accounts(&self) -> Result<Vec<Account>> {
        Ok(self.client.get_accounts().await?)
    }

    /// Get balance of coins with bond denom for the given address, together with a proof,
    /// and verify the returned balance against the corresponding block's app hash.
    ///
    /// NOTE: the balance returned is the balance reported by the parent block of
    /// the provided header. This is due to the fact that for block N, the block's
    /// app hash. is the result of applying the previous block's transaction list.
    ///
    /// `header` argument is a json encoded [`ExtendedHeader`]
    ///
    /// [`ExtendedHeader`]: https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    pub async fn get_verified_balance(&self, address: &Address, header: &str) -> Result<Coin> {
        let header: ExtendedHeader = serde_json::from_str(header)
            .map_err(|e| GrpcClientError::UniffiConversionError { msg: e.to_string() })?;
        Ok(self.client.get_verified_balance(address, &header).await?)
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

    /// Issue a direct ABCI query to the application
    async fn abci_query(
        &self,
        data: &[u8],
        path: &str,
        height: u64,
        prove: bool,
    ) -> Result<AbciQueryResponse> {
        Ok(self.client.abci_query(data, path, height, prove).await?)
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

    /// estimate_gas_price takes a transaction priority and estimates the gas price based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64> {
        Ok(self.client.estimate_gas_price(priority).await?)
    }

    /// estimate_gas_price_and_usage takes a transaction priority and a transaction bytes
    /// and estimates the gas price and the gas used for that transaction.
    ///
    /// The gas price estimation is based on the gas prices of the transactions in the last five blocks.
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    ///
    /// The gas used is estimated using the state machine simulation.
    async fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> Result<GasEstimate> {
        Ok(self
            .client
            .estimate_gas_price_and_usage(priority, tx_bytes)
            .await?)
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
