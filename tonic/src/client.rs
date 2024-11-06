use tonic::service::Interceptor;
use tonic::transport::Channel;

use celestia_proto::celestia::blob::v1::query_client::QueryClient as BlobQueryClient;
use celestia_proto::cosmos::auth::v1beta1::query_client::QueryClient as AuthQueryClient;
use celestia_proto::cosmos::base::node::v1beta1::service_client::ServiceClient as ConfigServiceClient;
use celestia_proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient;
use celestia_proto::cosmos::tx::v1beta1::service_client::ServiceClient as TxServiceClient;

use celestia_tendermint_proto::v0_34::types::BlobTx;
use celestia_types::auth::{AuthParams, BaseAccount};
use celestia_types::blob::BlobParams;

use grpc_method::grpc_method;

use crate::types::tx::{GetTxResponse, TxResponse};
use crate::types::Block;
use crate::types::{FromGrpcResponse, IntoGrpcParam};
use crate::Error;

pub use celestia_proto::cosmos::tx::v1beta1::BroadcastMode;

/// Struct wrapping all the tonic types and doing type conversion behind the scenes.
pub struct GrpcClient<I>
where
    I: Interceptor,
{
    grpc_channel: Channel,
    auth_interceptor: I,
}

impl<I> GrpcClient<I>
where
    I: Interceptor + Clone,
{
    /// Create a new client out of channel and optional auth
    pub fn new(grpc_channel: Channel, auth_interceptor: I) -> Self {
        Self {
            grpc_channel,
            auth_interceptor,
        }
    }

    /// Get Minimum Gas price
    #[grpc_method(ConfigServiceClient::config)]
    async fn get_min_gas_price(&mut self) -> Result<f64, Error>;

    /// Get latest block
    #[grpc_method(TendermintServiceClient::get_latest_block)]
    async fn get_latest_block(&mut self) -> Result<Block, Error>;

    /// Get block by height
    #[grpc_method(TendermintServiceClient::get_block_by_height)]
    async fn get_block_by_height(&mut self, height: i64) -> Result<Block, Error>;

    // TODO get_node_info
    // make_method!(TendermintServiceClient::get_node_info; get_node_info() -> NodeInfo);

    /// Get blob params
    #[grpc_method(BlobQueryClient::params)]
    async fn get_blob_params(&mut self) -> Result<BlobParams, Error>;

    /// Get auth params
    #[grpc_method(AuthQueryClient::params)]
    async fn get_auth_params(&mut self) -> Result<AuthParams, Error>;

    /// Get account
    #[grpc_method(AuthQueryClient::account)]
    async fn get_account(&mut self, account: String) -> Result<BaseAccount, Error>;

    // TODO: pagination?
    /// Get accounts
    #[grpc_method(AuthQueryClient::accounts)]
    async fn get_accounts(&mut self) -> Result<Vec<BaseAccount>, Error>;

    /// Broadcast Tx
    #[grpc_method(TxServiceClient::broadcast_tx)]
    async fn broadcast_tx(
        &mut self,
        blob_tx: BlobTx,
        mode: BroadcastMode,
    ) -> Result<TxResponse, Error>;

    /// Get Tx
    #[grpc_method(TxServiceClient::get_tx)]
    async fn get_tx(&mut self, hash: String) -> Result<GetTxResponse, Error>;
}
