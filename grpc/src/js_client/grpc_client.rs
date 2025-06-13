use wasm_bindgen::prelude::*;

use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::state::auth::JsAuthParams;
use celestia_types::state::auth::JsBaseAccount;
use celestia_types::state::{Bech32Address, TxResponse};

use crate::grpc::GetTxResponse;
use crate::grpc::TxStatusResponse;
use crate::grpc::{GasInfo, JsBroadcastMode, JsCoin};
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
    pub async fn new(url: String) -> Result<Self> {
        Ok(GrpcClient {
            client: InnerClient::with_grpcweb_url(url),
        })
    }

    /// Get auth params
    pub async fn get_auth_params(&self) -> Result<JsAuthParams> {
        Ok(self.client.get_auth_params().await?.into())
    }

    /// Get account
    pub async fn get_account(&self, account: Bech32Address) -> Result<JsBaseAccount> {
        Ok(self.client.get_account(&account.try_into()?).await?.into())
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
    pub async fn get_balance(&self, address: Bech32Address, denom: String) -> Result<JsCoin> {
        Ok(self
            .client
            .get_balance(&address.try_into()?, denom)
            .await?
            .into())
    }

    /// Get balance of all coins
    pub async fn get_all_balances(&self, address: Bech32Address) -> Result<Vec<JsCoin>> {
        Ok(self
            .client
            .get_all_balances(&address.try_into()?)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get balance of all spendable coins
    pub async fn get_spendable_balances(&self, address: Bech32Address) -> Result<Vec<JsCoin>> {
        Ok(self
            .client
            .get_spendable_balances(&address.try_into()?)
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
        Ok(self.client.get_min_gas_price().await?)
    }

    /// Get latest block
    pub async fn get_latest_block(&self) -> Result<Block> {
        Ok(self.client.get_latest_block().await?)
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: i64) -> Result<Block> {
        Ok(self.client.get_block_by_height(height).await?)
    }

    /// Broadcast prepared and serialised transaction
    pub async fn broadcast_tx(
        &self,
        tx_bytes: Vec<u8>,
        mode: JsBroadcastMode,
    ) -> Result<TxResponse> {
        Ok(self.client.broadcast_tx(tx_bytes, mode.into()).await?)
    }

    /// Get Tx
    pub async fn get_tx(&self, hash: &str) -> Result<GetTxResponse> {
        Ok(self.client.get_tx(hash.parse()?).await?)
    }

    /// Simulate prepared and serialised transaction, returning simulated gas usage
    pub async fn simulate(&self, tx_bytes: Vec<u8>) -> Result<GasInfo> {
        Ok(self.client.simulate(tx_bytes).await?)
    }

    /// Get blob params
    pub async fn get_blob_params(&self) -> Result<BlobParams> {
        Ok(self.client.get_blob_params().await?)
    }

    /// Get status of the transaction
    pub async fn tx_status(&self, hash: &str) -> Result<TxStatusResponse> {
        Ok(self.client.tx_status(hash.parse()?).await?)
    }
}
