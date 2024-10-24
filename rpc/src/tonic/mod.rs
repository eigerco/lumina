use celestia_proto::cosmos::base::node::v1beta1::{
    service_client::ServiceClient as ConfigServiceClient, ConfigRequest,
};
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::Status;

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

    #[error("Failed to parse response")]
    FailedToParseResponse,
}

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
    pub fn new(grpc_channel: Channel, auth_interceptor: I) -> Self {
        Self {
            grpc_channel,
            auth_interceptor,
        }
    }
    pub async fn get_min_gas_price(&self) -> Result<f64, Error> {
        const UNITS_SUFFIX: &str = "utia";

        let mut min_gas_price_client = ConfigServiceClient::with_interceptor(
            self.grpc_channel.clone(),
            self.auth_interceptor.clone(),
        );
        let response = min_gas_price_client.config(ConfigRequest {}).await;

        let min_gas_price_with_suffix = response?.into_inner().minimum_gas_price;
        let min_gas_price_str = min_gas_price_with_suffix
            .strip_suffix(UNITS_SUFFIX)
            .ok_or(Error::FailedToParseResponse)?;
        let min_gas_price = min_gas_price_str
            .parse::<f64>()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(min_gas_price)
    }
}
