use celestia_proto::cosmos::base::tendermint::v1beta1::{
    GetBlockByHeightRequest, GetBlockByHeightResponse, GetLatestBlockRequest,
    GetLatestBlockResponse,
};
use celestia_types::block::Block;

use crate::grpc::{make_empty_params, FromGrpcResponse, IntoGrpcParam};
use crate::{Error, Result};

impl FromGrpcResponse<Block> for GetBlockByHeightResponse {
    fn try_from_response(self) -> Result<Block> {
        Ok(self.block.ok_or(Error::FailedToParseResponse)?.try_into()?)
    }
}

impl FromGrpcResponse<Block> for GetLatestBlockResponse {
    fn try_from_response(self) -> Result<Block> {
        Ok(self.block.ok_or(Error::FailedToParseResponse)?.try_into()?)
    }
}

impl IntoGrpcParam<GetBlockByHeightRequest> for i64 {
    fn into_parameter(self) -> GetBlockByHeightRequest {
        GetBlockByHeightRequest { height: self }
    }
}

make_empty_params!(GetLatestBlockRequest);
