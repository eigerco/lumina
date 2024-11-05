use tonic::service::Interceptor;
use tonic::transport::Channel;

use celestia_proto::celestia::blob::v1 as blob;
use celestia_proto::cosmos::auth::v1beta1 as auth;
use celestia_proto::cosmos::base::node::v1beta1 as config;
use celestia_proto::cosmos::base::tendermint::v1beta1 as tendermint;
use celestia_proto::cosmos::tx::v1beta1 as tx;
use celestia_tendermint_proto::v0_34::types::BlobTx;
use celestia_types::auth::{AuthParams, BaseAccount};
use celestia_types::blob::BlobParams;

use crate::types::tx::{GetTxResponse, TxResponse};
use crate::types::Block;
use crate::types::{FromGrpcResponse, IntoGrpcParam};
use crate::Error;

pub struct GrpcClient<I>
where
    I: Interceptor,
{
    grpc_channel: Channel,
    auth_interceptor: I,
}

// macro takes a path to an appropriate generated gRPC method and a desired function signature.
// If parameters need to be converted, they should implement [`types::IntoGrpcParam`] into
// appropriate type and return type is converted using TryFrom
//
// One limitation is that it expects gRPC method to be provided in exactly 4 `::` delimited
// segments, requiring importing the proto module with `as`. This might be possible to overcome
// by rewriting the macro as tt-muncher, but it'd increase its complexity significantly
macro_rules! make_method2 {
    ($path:ident :: $client_module:ident :: $client_struct:ident :: $method:ident; $name:ident ( $( $param:ident : $param_type:ty ),* ) -> $ret:ty) => {
        pub async fn $name(&mut self, $($param: $param_type),*) -> $ret {
            let mut client = $path::$client_module::$client_struct::with_interceptor(
                self.grpc_channel.clone(),
                self.auth_interceptor.clone(),
            );
            let request = ::tonic::Request::new(( $($param),* ).into_parameter());
            let response = client.$method(request).await;

            Ok(response?.into_inner().try_from_response()?)
        }
    };
}

impl<I> GrpcClient<I>
where
    I: Interceptor + Clone,
{
    pub fn new(grpc_channel: Channel, auth_interceptor: I) -> Self {
        Self {
            grpc_channel,
            auth_interceptor,
        }
    }

    make_method2!(config::service_client::ServiceClient::config; get_min_gas_price() -> Result<f64, Error>);

    make_method2!(tendermint::service_client::ServiceClient::get_latest_block; get_latest_block() -> Result<Block, Error>);
    make_method2!(tendermint::service_client::ServiceClient::get_block_by_height; get_block_by_height(height:i64) -> Result<Block, Error>);
    // TODO get_node_info
    // make_method!(tendermint::service_client::ServiceClient::get_node_info; get_node_info() -> NodeInfo);

    make_method2!(blob::query_client::QueryClient::params; get_blob_params() -> Result<BlobParams, Error>);

    make_method2!(auth::query_client::QueryClient::params; get_auth_params() -> Result<AuthParams, Error>);
    make_method2!(auth::query_client::QueryClient::account; get_account(account: String) -> Result<BaseAccount, Error>);
    // TODO: pagination?
    make_method2!(auth::query_client::QueryClient::accounts; get_accounts() -> Result<Vec<BaseAccount>, Error>);

    make_method2!(tx::service_client::ServiceClient::broadcast_tx; broadcast_tx(blob_tx: BlobTx, mode: tx::BroadcastMode) -> Result<TxResponse, Error>);
    make_method2!(tx::service_client::ServiceClient::get_tx; get_tx(hash: String) -> Result<GetTxResponse, Error>);
}
