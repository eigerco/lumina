use celestia_proto::cosmos::staking::v1beta1::{
    Delegation as RawDelegation, QueryDelegationRequest,
    QueryDelegationResponse as RawQueryDelegationResponse,
};
use celestia_types::state::{AccAddress, Coin, QueryDelegationResponse, ValAddress};

use crate::grpc::{FromGrpcResponse, IntoGrpcParam};
use crate::{Error, Result};

impl IntoGrpcParam<QueryDelegationRequest> for (&AccAddress, &ValAddress) {
    fn into_parameter(self) -> QueryDelegationRequest {
        QueryDelegationRequest {
            delegator_addr: self.0.to_string(),
            validator_addr: self.1.to_string(),
        }
    }
}

impl FromGrpcResponse<QueryDelegationResponse> for RawQueryDelegationResponse {
    fn try_from_response(self) -> Result<QueryDelegationResponse> {
        Ok(self.try_into()?)
    }
}
