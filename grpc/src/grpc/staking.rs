use celestia_proto::cosmos::staking::v1beta1::{
    QueryDelegationRequest, QueryDelegationResponse as RawQueryDelegationResponse,
    QueryUnbondingDelegationRequest,
    QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse,
};
use celestia_types::state::{
    AccAddress, Coin, QueryDelegationResponse, QueryUnbondingDelegationResponse, ValAddress,
};

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
        todo!();
    }
}
