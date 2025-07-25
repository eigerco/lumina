//! Types and client for the celestia grpc

use std::fmt;

use bytes::Bytes;
use celestia_grpc_macros::grpc_method;
use celestia_proto::celestia::blob::v1::query_client::QueryClient as BlobQueryClient;
use celestia_proto::celestia::core::v1::gas_estimation::gas_estimator_client::GasEstimatorClient;
use celestia_proto::celestia::core::v1::tx::tx_client::TxClient as TxStatusClient;
use celestia_proto::cosmos::auth::v1beta1::query_client::QueryClient as AuthQueryClient;
use celestia_proto::cosmos::bank::v1beta1::query_client::QueryClient as BankQueryClient;
pub use celestia_proto::cosmos::base::abci::v1beta1::GasInfo;
use celestia_proto::cosmos::base::node::v1beta1::service_client::ServiceClient as ConfigServiceClient;
use celestia_proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient;
use celestia_proto::cosmos::tx::v1beta1::service_client::ServiceClient as TxServiceClient;
use celestia_types::blob::BlobParams;
use celestia_types::block::Block;
use celestia_types::consts::appconsts;
use celestia_types::hash::Hash;
use celestia_types::state::auth::{Account, AuthParams};
use celestia_types::state::AbciQueryResponse;
use celestia_types::state::{
    AccAddress, Address, AddressTrait, Coin, ErrorCode, TxResponse, BOND_DENOM,
};
use celestia_types::ExtendedHeader;
use http_body::Body;
use tonic::body::BoxBody;
use tonic::client::GrpcService;

use crate::abci_proofs::ProofChain;
use crate::{Error, Result};

// cosmos.auth
mod auth;
// cosmos.bank
mod bank;
// celestia.core.gas_estimation
mod gas_estimation;
// cosmos.base.node
mod node;
// cosmos.base.tendermint
mod tendermint;
// celestia.core.tx
mod celestia_tx;
// celestia.blob
mod blob;
// cosmos.tx
mod cosmos_tx;

pub use crate::grpc::celestia_tx::{TxStatus, TxStatusResponse};
pub use crate::grpc::cosmos_tx::{BroadcastMode, GetTxResponse};
pub use crate::grpc::gas_estimation::{GasEstimate, TxPriority};

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use crate::grpc::cosmos_tx::JsBroadcastMode;

/// Error convertible to std, used by grpc transports
pub type StdError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Struct wrapping all the tonic types and doing type conversion behind the scenes.
pub struct GrpcClient<T> {
    transport: T,
}

impl<T> GrpcClient<T> {
    /// Get the underlying transport.
    pub fn into_inner(self) -> T {
        self.transport
    }
}

impl<T> GrpcClient<T>
where
    T: GrpcService<BoxBody> + Clone,
    T::Error: Into<StdError>,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
{
    /// Create a new client wrapping given transport
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    // cosmos.auth

    /// Get auth params
    #[grpc_method(AuthQueryClient::params)]
    async fn get_auth_params(&self) -> Result<AuthParams>;

    /// Get account
    #[grpc_method(AuthQueryClient::account)]
    async fn get_account(&self, account: &AccAddress) -> Result<Account>;

    /// Get accounts
    #[grpc_method(AuthQueryClient::accounts)]
    async fn get_accounts(&self) -> Result<Vec<Account>>;

    // cosmos.bank

    /// Get balance of coins with [`BOND_DENOM`] for the given address, together with a proof,
    /// and verify the returned balance against the corresponding block's [`AppHash`].
    ///
    /// NOTE: the balance returned is the balance reported by the parent block of
    /// the provided header. This is due to the fact that for block N, the block's
    /// [`AppHash`] is the result of applying the previous block's transaction list.
    ///
    /// [`AppHash`]: ::tendermint::hash::AppHash
    pub async fn get_verified_balance(
        &self,
        address: &Address,
        header: &ExtendedHeader,
    ) -> Result<Coin> {
        // construct the key for querying account's balance from bank state
        let mut prefixed_account_key = Vec::with_capacity(1 + 1 + appconsts::SIGNER_SIZE + 4);
        prefixed_account_key.push(0x02); // balances prefix
        prefixed_account_key.push(address.as_bytes().len() as u8); // address length
        prefixed_account_key.extend_from_slice(address.as_bytes()); // address
        prefixed_account_key.extend_from_slice(BOND_DENOM.as_bytes()); // denom

        // abci queries are possible only from 2nd block, but we can rely on the node to
        // return an error if that is the case. We only want to prevent height == 0 here,
        // because that will make node interpret that as a network head. It would also fail,
        // on proof verification, but it would be harder to debug as the message would just
        // say that computed root is different than expected.
        let height = 1.max(header.height().value().saturating_sub(1));

        let response = self
            .abci_query(&prefixed_account_key, "store/bank/key", height, true)
            .await?;
        if response.code != ErrorCode::Success {
            return Err(Error::AbciQuery(response.code, response.log));
        }

        // If account doesn't exist yet, just return 0
        if response.value.is_empty() {
            return Ok(Coin::utia(0));
        }

        // NOTE: don't put `ProofChain` directly in the AbciQueryResponse, because
        // it supports only small subset of proofs that are required for the balance
        // queries
        let proof: ProofChain = response.proof_ops.unwrap_or_default().try_into()?;
        proof.verify_membership(
            &header.header.app_hash,
            [prefixed_account_key.as_slice(), b"bank"],
            &response.value,
        )?;

        let amount = str::from_utf8(&response.value)
            .map_err(|_| Error::FailedToParseResponse)?
            .parse()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(Coin::utia(amount))
    }

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

    /// Issue a direct ABCI query to the application
    #[grpc_method(TendermintServiceClient::abci_query)]
    async fn abci_query(
        &self,
        data: impl AsRef<[u8]>,
        path: impl Into<String>,
        height: u64,
        prove: bool,
    ) -> Result<AbciQueryResponse>;

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

    // celestia.core.gas_estimation

    /// Estimate gas price for given transaction priority based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    #[grpc_method(GasEstimatorClient::estimate_gas_price)]
    async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64>;

    /// Estimate gas price for transaction with given priority and estimate gas usage for
    /// privded serialised transaction.
    ///
    /// The gas price estimation is based on the gas prices of the transactions in the last five blocks.
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    ///
    /// The gas used is estimated using the state machine simulation.
    #[grpc_method(GasEstimatorClient::estimate_gas_price_and_usage)]
    async fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> Result<GasEstimate>;
}

impl<T> fmt::Debug for GrpcClient<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GrpcClient { .. }")
    }
}

pub(crate) trait FromGrpcResponse<T> {
    fn try_from_response(self) -> Result<T>;
}

pub(crate) trait IntoGrpcParam<T> {
    fn into_parameter(self) -> T;
}

macro_rules! make_empty_params {
    ($request_type:ident) => {
        impl crate::grpc::IntoGrpcParam<$request_type> for () {
            fn into_parameter(self) -> $request_type {
                $request_type {}
            }
        }
    };
}

pub(crate) use make_empty_params;
