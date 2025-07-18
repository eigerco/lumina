use celestia_proto::cosmos::base::tendermint::v1beta1::{
    AbciQueryRequest, AbciQueryResponse as RawAbciQueryResponse, GetBlockByHeightRequest,
    GetBlockByHeightResponse, GetLatestBlockRequest, GetLatestBlockResponse,
};
use celestia_types::block::Block;
use celestia_types::state::AbciQueryResponse;

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

impl<Data, Path> IntoGrpcParam<AbciQueryRequest> for (Data, Path, u64, bool)
where
    Data: AsRef<[u8]>,
    Path: Into<String>,
{
    fn into_parameter(self) -> AbciQueryRequest {
        AbciQueryRequest {
            data: self.0.as_ref().to_vec(),
            path: self.1.into(),
            height: self.2 as i64,
            prove: self.3,
        }
    }
}

impl FromGrpcResponse<AbciQueryResponse> for RawAbciQueryResponse {
    fn try_from_response(self) -> Result<AbciQueryResponse> {
        Ok(self.try_into()?)
    }
}

make_empty_params!(GetLatestBlockRequest);
