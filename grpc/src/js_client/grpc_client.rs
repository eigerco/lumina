use wasm_bindgen::prelude::*;

use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::state::auth::JsAuthParams;
use celestia_types::state::auth::JsBaseAccount;
use celestia_types::state::{JsCoin, TxResponse};

use crate::grpc::GetTxResponse;
use crate::grpc::TxStatusResponse;
use crate::grpc::{GasInfo, JsBroadcastMode};
use crate::Result;

type InnerClient = crate::GrpcClient<tonic_web_wasm_client::Client>;

/// Celestia GRPC client
#[wasm_bindgen]
pub struct GrpcClient {
    client: InnerClient,
}

#[wasm_bindgen]
impl GrpcClient {
    /// Create a new client connected with the given `url`
    pub async fn new(url: &str) -> Result<Self> {
        Ok(GrpcClient {
            client: InnerClient::with_grpcweb_url(url),
        })
    }

    /// Get auth params
    pub async fn get_auth_params(&self) -> Result<JsAuthParams> {
        Ok(self.client.get_auth_params().await?.into())
    }

    /// Get account
    pub async fn get_account(&self, account: &str) -> Result<JsBaseAccount> {
        Ok(self.client.get_account(&account.parse()?).await?.into())
    }

    /// Get accounts
    pub async fn get_accounts(&self) -> Result<Vec<JsBaseAccount>> {
        Ok(self
            .client
            .get_accounts()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get balance of coins with given denom
    pub async fn get_balance(&self, address: &str, denom: &str) -> Result<JsCoin> {
        Ok(self
            .client
            .get_balance(&address.parse()?, denom)
            .await?
            .into())
    }

    /// Get balance of all coins
    pub async fn get_all_balances(&self, address: &str) -> Result<Vec<JsCoin>> {
        Ok(self
            .client
            .get_all_balances(&address.parse()?)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get balance of all spendable coins
    pub async fn get_spendable_balances(&self, address: &str) -> Result<Vec<JsCoin>> {
        Ok(self
            .client
            .get_spendable_balances(&address.parse()?)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get total supply
    pub async fn get_total_supply(&self) -> Result<Vec<JsCoin>> {
        Ok(self
            .client
            .get_total_supply()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get Minimum Gas price
    pub async fn get_min_gas_price(&self) -> Result<f64> {
        self.client.get_min_gas_price().await
    }

    /// Get latest block
    pub async fn get_latest_block(&self) -> Result<Block> {
        self.client.get_latest_block().await
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: i64) -> Result<Block> {
        self.client.get_block_by_height(height).await
    }

    /// Broadcast prepared and serialised transaction
    pub async fn broadcast_tx(
        &self,
        tx_bytes: Vec<u8>,
        mode: &JsBroadcastMode,
    ) -> Result<TxResponse> {
        self.client.broadcast_tx(tx_bytes, (*mode).into()).await
    }

    /// Get Tx
    pub async fn get_tx(&self, hash: &str) -> Result<GetTxResponse> {
        self.client.get_tx(hash.parse()?).await
    }

    /// Simulate prepared and serialised transaction, returning simulated gas usage
    pub async fn simulate(&self, tx_bytes: Vec<u8>) -> Result<GasInfo> {
        self.client.simulate(tx_bytes).await
    }

    /// Get blob params
    pub async fn get_blob_params(&self) -> Result<BlobParams> {
        self.client.get_blob_params().await
    }

    /// Get status of the transaction
    pub async fn tx_status(&self, hash: &str) -> Result<TxStatusResponse> {
        self.client.tx_status(hash.parse()?).await
    }
}
