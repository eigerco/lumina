use celestia_proto::celestia::blob::v1::{
    QueryParamsRequest as QueryBlobParamsRequest, QueryParamsResponse as QueryBlobParamsResponse,
};
use celestia_types::blob::BlobParams;

use crate::grpc::{FromGrpcResponse, make_empty_params};
use crate::{Error, Result};

impl FromGrpcResponse<BlobParams> for QueryBlobParamsResponse {
    fn try_from_response(self) -> Result<BlobParams> {
        let params = self.params.ok_or(Error::FailedToParseResponse)?;
        Ok(BlobParams {
            gas_per_blob_byte: params.gas_per_blob_byte,
            gov_max_square_size: params.gov_max_square_size,
        })
    }
}

make_empty_params!(QueryBlobParamsRequest);
