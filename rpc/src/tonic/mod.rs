use std::convert::Infallible;

use celestia_types::auth::{AuthParams, BaseAccount};
use celestia_types::blob::BlobParams;

use celestia_proto::celestia::blob::v1 as blob;
use celestia_proto::cosmos::auth::v1beta1 as auth;
use celestia_proto::cosmos::base::node::v1beta1 as config;
use celestia_proto::cosmos::base::tendermint::v1beta1 as tendermint;
use celestia_proto::cosmos::tx::v1beta1 as tx;

use cosmrs::ErrorReport;

use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::Status;

pub mod types;

use types::tx::{GetTxResponse, TxResponse};
use types::Block;
use types::{FromGrpcResponse, IntoGrpcParam};

use celestia_tendermint_proto::v0_34::types::BlobTx;

/*
use celestia_proto::celestia::blob::v1::query_client::QueryClient;
use celestia_tendermint_proto::v0_34::types::{
    Blob as PbBlob,
    BlobTx,
};

use celestia_proto::{
    celestia::blob::v1::{
        query_client::QueryClient as BlobQueryClient,
        MsgPayForBlobs,
        QueryParamsRequest as QueryBlobParamsRequest,
        Params as BlobParams,
    },
    cosmos::{
        auth::v1beta1::{
            query_client::QueryClient as AuthQueryClient,
            BaseAccount,
            Params as AuthParams,
            QueryAccountRequest,
            QueryAccountResponse,
            QueryParamsRequest as QueryAuthParamsRequest,
        },
        base::{
            node::v1beta1::{
                service_client::ServiceClient as MinGasPriceClient,
                ConfigRequest as MinGasPriceRequest,
                ConfigResponse as MinGasPriceResponse,
            },
            tendermint::v1beta1::{
                service_client::ServiceClient as TendermintServiceClient,
                GetNodeInfoRequest,
            },
            v1beta1::Coin,
        },
        crypto::secp256k1,
        tx::v1beta1::{
            mode_info::{
                Single,
                Sum,
            },
            service_client::ServiceClient as TxClient,
            AuthInfo,
            BroadcastMode,
            BroadcastTxRequest,
            BroadcastTxResponse,
            Fee,
            GetTxRequest,
            GetTxResponse,
            ModeInfo,
            SignDoc,
            SignerInfo,
            Tx,
            TxBody,
        },
    },
};
*/

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    TonicError(#[from] Status),

    #[error(transparent)]
    TendermintError(#[from] celestia_tendermint::Error),

    #[error(transparent)]
    CosmrsError(#[from] ErrorReport),

    #[error(transparent)]
    TendermintProtoError(#[from] celestia_tendermint_proto::Error),

    #[error("Failed to parse response")]
    FailedToParseResponse,

    #[error("Unexpected response type")]
    UnexpectedResponseType(String),

    /// Unreachable. Added to appease try_into conversion for GrpcClient method macro
    #[error(transparent)]
    Infallible(#[from] Infallible),
}

pub struct GrpcClient<I>
where
    I: Interceptor,
{
    grpc_channel: Channel,
    auth_interceptor: I,
}

/*
macro_rules! make_method {
    ($name:ident, $service_client:ident, $method:ident, $param:ty, $ret:ty) => {
        pub async fn $name(&mut self, param: $param) -> Result<$ret, Error> {
            let mut service_client =
                $service_client::service_client::ServiceClient::with_interceptor(
                    self.grpc_channel.clone(),
                    self.auth_interceptor.clone(),
                );
            let response = service_client.$method(param).await;

            Ok(response?.into_inner().try_into()?)
        }
    };
    ($name:ident, $service_client:ident, $method:ident, $ret:ty) => {
        pub async fn $name(&mut self) -> Result<$ret, Error> {
            let mut service_client =
                $service_client::service_client::ServiceClient::with_interceptor(
                    self.grpc_channel.clone(),
                    self.auth_interceptor.clone(),
                );
            let response = service_client
                .$method(::tonic::Request::new(Default::default()))
                .await;

            Ok(response?.into_inner().try_into()?)
        }
    };
}

macro_rules! make_query {
    ($name:ident, $service_client:ident, $method:ident, $param:ty, $ret:ty) => {
        pub async fn $name(&mut self, param: $param) -> Result<$ret, Error> {
            let mut client = $service_client::query_client::QueryClient::with_interceptor(
                self.grpc_channel.clone(),
                self.auth_interceptor.clone(),
            );
            let response = client.$method(param).await;

            Ok(response?.into_inner().try_into()?)
        }
    };
    ($name:ident, $service_client:ident, $method:ident, $ret:ty) => {
        pub async fn $name(&mut self) -> Result<$ret, Error> {
            let mut client = $service_client::query_client::QueryClient::with_interceptor(
                self.grpc_channel.clone(),
                self.auth_interceptor.clone(),
            );
            let response = client
                .$method(::tonic::Request::new(Default::default()))
                .await;

            Ok(response?.into_inner().try_into()?)
        }
    };
}
*/

/*
macro_rules! make_method {
    ($path:ident :: $client_module:ident :: $client_struct:ident :: $method:ident; $name:ident ( $param:ty ) -> $ret:ty) => {
        pub async fn $name(&mut self, param: $param) -> Result<$ret, Error> {
            let mut client = $path::$client_module::$client_struct::with_interceptor(
                self.grpc_channel.clone(),
                self.auth_interceptor.clone(),
            );
            let response = client.$method(param.into_parameter()).await;

            Ok(response?.into_inner().try_from_response()?)
        }
    };
    ($path:ident :: $client_module:ident :: $client_struct:ident :: $method:ident; $name:ident () -> $ret:ty) => {
        pub async fn $name(&mut self) -> Result<$ret, Error> {
            let mut client = $path::$client_module::$client_struct::with_interceptor(
                self.grpc_channel.clone(),
                self.auth_interceptor.clone(),
            );
            let response = client
                .$method(::tonic::Request::new(Default::default()))
                .await;

            Ok(response?.into_inner().try_from_response()?)
        }
    };
}
*/

// macro takes a path to an appropriate generated gRPC method and a desired function signature.
// If parameters need to be converted, they should implement [`types::IntoGrpcParam`] into
// appropriate type and return type is converted using TryFrom
//
// One limitation is that it expects gRPC method to be provided in exactly 4 `::` delimited
// segments, requiring importing the proto module with `as`. This might be possible to overcome
// by rewriting the macro as tt-muncher, but it'd increase its complexity significantly
macro_rules! make_method2 {
    ($path:ident :: $client_module:ident :: $client_struct:ident :: $method:ident; $name:ident ( $( $param:ident : $param_type:ty ),* ) -> $ret:ty) => {
        pub async fn $name(&mut self, $($param: $param_type),*) -> Result<$ret, Error> {
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

/*
macro_rules! mm {
    ($prefix:ident $(:: $tail:path)+; $name:ident ( $param:ty ) -> $ret:ty ) => {
        mm!($prefix :: ; $($tail),* | $name($param) -> $ret);
    };

    ($($module:ident ::)+ ; $head:ident, $($tail:ident),+ | $name:ident ($param:ty) -> $ret:ty ) => {

    };

    //(@resolved $module:path, $client_module:ident, $client_struct:ident, $method:ident; )
}

mm!(tendermint::service_client::ServiceClient::get_block_by_height; get_block_by_height(i64) -> Block);
*/

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

    make_method2!(config::service_client::ServiceClient::config; get_min_gas_price() -> f64);

    make_method2!(tendermint::service_client::ServiceClient::get_latest_block; get_latest_block() -> Block);
    make_method2!(tendermint::service_client::ServiceClient::get_block_by_height; get_block_by_height(height:i64) -> Block);
    // TODO get_node_info
    // make_method!(tendermint::service_client::ServiceClient::get_node_info; get_node_info() -> NodeInfo);

    make_method2!(blob::query_client::QueryClient::params; get_blob_params() -> BlobParams);

    make_method2!(auth::query_client::QueryClient::params; get_auth_params() -> AuthParams);
    make_method2!(auth::query_client::QueryClient::account; get_account(account: String) -> BaseAccount);

    make_method2!(tx::service_client::ServiceClient::broadcast_tx; broadcast_tx(blob_tx: BlobTx, mode: tx::BroadcastMode) -> TxResponse);
    make_method2!(tx::service_client::ServiceClient::get_tx; get_tx(hash: String) -> GetTxResponse);
}
