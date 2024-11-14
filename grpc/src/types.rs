//! Custom types and wrappers needed by gRPC

use prost::Message;

use celestia_proto::celestia::blob::v1::{
    QueryParamsRequest as QueryBlobParamsRequest, QueryParamsResponse as QueryBlobParamsResponse,
};
use celestia_proto::cosmos::base::node::v1beta1::{ConfigRequest, ConfigResponse};
use celestia_proto::cosmos::base::tendermint::v1beta1::{
    GetBlockByHeightRequest, GetBlockByHeightResponse, GetLatestBlockRequest,
    GetLatestBlockResponse,
};
use celestia_proto::cosmos::tx::v1beta1::{BroadcastMode, BroadcastTxRequest, GetTxRequest};
use celestia_tendermint::block::Block;
use celestia_tendermint_proto::v0_34::types::BlobTx;
use celestia_types::blob::BlobParams;

use crate::Error;

/// types related to authorisation
pub mod auth;
/// types related to transaction querying and submission
pub mod tx;

macro_rules! make_empty_params {
    ($request_type:ident) => {
        impl IntoGrpcParam<$request_type> for () {
            fn into_parameter(self) -> $request_type {
                $request_type {}
            }
        }
    };
}

pub(crate) use make_empty_params;

pub(crate) trait FromGrpcResponse<T> {
    fn try_from_response(self) -> Result<T, Error>;
}

pub(crate) trait IntoGrpcParam<T> {
    fn into_parameter(self) -> T;
}

impl FromGrpcResponse<BlobParams> for QueryBlobParamsResponse {
    fn try_from_response(self) -> Result<BlobParams, Error> {
        let params = self.params.ok_or(Error::FailedToParseResponse)?;
        Ok(BlobParams {
            gas_per_blob_byte: params.gas_per_blob_byte,
            gov_max_square_size: params.gov_max_square_size,
        })
    }
}

impl FromGrpcResponse<Block> for GetBlockByHeightResponse {
    fn try_from_response(self) -> Result<Block, Error> {
        Ok(self.block.ok_or(Error::FailedToParseResponse)?.try_into()?)
    }
}

impl FromGrpcResponse<Block> for GetLatestBlockResponse {
    fn try_from_response(self) -> Result<Block, Error> {
        Ok(self.block.ok_or(Error::FailedToParseResponse)?.try_into()?)
    }
}

impl FromGrpcResponse<f64> for ConfigResponse {
    fn try_from_response(self) -> Result<f64, Error> {
        const UNITS_SUFFIX: &str = "utia";

        let min_gas_price_with_suffix = self.minimum_gas_price;
        let min_gas_price_str = min_gas_price_with_suffix
            .strip_suffix(UNITS_SUFFIX)
            .ok_or(Error::FailedToParseResponse)?;
        let min_gas_price = min_gas_price_str
            .parse::<f64>()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(min_gas_price)
    }
}

impl IntoGrpcParam<GetBlockByHeightRequest> for i64 {
    fn into_parameter(self) -> GetBlockByHeightRequest {
        GetBlockByHeightRequest { height: self }
    }
}

impl IntoGrpcParam<BroadcastTxRequest> for (BlobTx, BroadcastMode) {
    fn into_parameter(self) -> BroadcastTxRequest {
        let (blob_tx, mode) = self;
        BroadcastTxRequest {
            tx_bytes: blob_tx.encode_to_vec(),
            mode: mode.into(),
        }
    }
}

impl IntoGrpcParam<GetTxRequest> for String {
    fn into_parameter(self) -> GetTxRequest {
        GetTxRequest { hash: self }
    }
}

make_empty_params!(GetLatestBlockRequest);
make_empty_params!(ConfigRequest);
make_empty_params!(QueryBlobParamsRequest);
