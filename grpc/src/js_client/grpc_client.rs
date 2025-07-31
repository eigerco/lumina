use wasm_bindgen::prelude::*;

use celestia_proto::tendermint_celestia_mods::rpc::grpc::StatusResponse;
use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::state::auth::{JsAuthParams, JsBaseAccount};
use celestia_types::state::{AbciQueryResponse, JsCoin, TxResponse};
use celestia_types::ExtendedHeader;

use crate::grpc::{GasInfo, GetTxResponse, JsBroadcastMode, TxStatusResponse};
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

    /// Get balance of coins with bond denom for the given address, together with a proof,
    /// and verify the returned balance against the corresponding block's app hash.
    ///
    /// NOTE: the balance returned is the balance reported by the parent block of
    /// the provided header. This is due to the fact that for block N, the block's
    /// app hash is the result of applying the previous block's transaction list.
    pub async fn get_verified_balance(
        &self,
        address: &str,
        header: &ExtendedHeader,
    ) -> Result<JsCoin> {
        Ok(self
            .client
            .get_verified_balance(&address.parse()?, header)
            .await?
            .into())
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

    /// Get node's minimum gas price
    pub async fn get_min_gas_price(&self) -> Result<f64> {
        self.client.get_min_gas_price().await
    }

    /// Get node's status
    ///
    /// Please note that this uses `tendermint.rpc.grpc.BlockAPI` from
    /// `celestia-core` fork of commetbft rather than `cosmos.base.node.v1beta1.Service`
    /// to get more celestia specific info.
    pub async fn get_node_status(&self) -> Result<StatusResponse> {
        self.client.get_node_status().await
    }

    /// Get latest block
    pub async fn get_latest_block(&self) -> Result<Block> {
        self.client.get_latest_block().await
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: i64) -> Result<Block> {
        self.client.get_block_by_height(height).await
    }

    /// Issue a direct ABCI query to the application
    pub async fn abci_query(
        &self,
        data: Vec<u8>,
        path: &str,
        height: u64,
        prove: bool,
    ) -> Result<AbciQueryResponse> {
        self.client.abci_query(data, path, height, prove).await
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
