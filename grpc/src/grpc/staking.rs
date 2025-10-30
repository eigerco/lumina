use celestia_proto::cosmos::staking::v1beta1::{
    QueryDelegationRequest, QueryDelegationResponse as RawQueryDelegationResponse,
    QueryRedelegationsRequest, QueryRedelegationsResponse as RawQueryRedelegationsResponse,
    QueryUnbondingDelegationRequest,
    QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse,
};
use celestia_types::state::{
    AccAddress, PageRequest, QueryDelegationResponse, QueryRedelegationsResponse,
    QueryUnbondingDelegationResponse, ValAddress,
};

use crate::Result;
use crate::grpc::{FromGrpcResponse, IntoGrpcParam};

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

impl IntoGrpcParam<QueryUnbondingDelegationRequest> for (&AccAddress, &ValAddress) {
    fn into_parameter(self) -> QueryUnbondingDelegationRequest {
        QueryUnbondingDelegationRequest {
            delegator_addr: self.0.to_string(),
            validator_addr: self.1.to_string(),
        }
    }
}

impl FromGrpcResponse<QueryUnbondingDelegationResponse> for RawQueryUnbondingDelegationResponse {
    fn try_from_response(self) -> Result<QueryUnbondingDelegationResponse> {
        Ok(self.try_into()?)
    }
}

impl IntoGrpcParam<QueryRedelegationsRequest>
    for (&AccAddress, &ValAddress, &ValAddress, Option<PageRequest>)
{
    fn into_parameter(self) -> QueryRedelegationsRequest {
        QueryRedelegationsRequest {
            delegator_addr: self.0.to_string(),
            src_validator_addr: self.1.to_string(),
            dst_validator_addr: self.2.to_string(),
            pagination: self.3,
        }
    }
}

impl FromGrpcResponse<QueryRedelegationsResponse> for RawQueryRedelegationsResponse {
    fn try_from_response(self) -> Result<QueryRedelegationsResponse> {
        Ok(self.try_into()?)
    }
}
