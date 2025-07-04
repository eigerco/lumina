use celestia_proto::cosmos::base::tendermint::v1beta1::{
    AbciQueryRequest, AbciQueryResponse, GetBlockByHeightRequest, GetBlockByHeightResponse,
    GetLatestBlockRequest, GetLatestBlockResponse,
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

impl IntoGrpcParam<AbciQueryRequest> for AbciQueryRequest {
    fn into_parameter(self) -> AbciQueryRequest {
        self
    }
}

// TODO: Wrap abci query response the same way as TxResponse
impl FromGrpcResponse<AbciQueryResponse> for AbciQueryResponse {
    fn try_from_response(self) -> Result<AbciQueryResponse> {
        Ok(self)
    }
}

make_empty_params!(GetLatestBlockRequest);
