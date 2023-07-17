use celestia_proto::cosmos::staking::v1beta1::QueryDelegationResponse as RawQueryDelegationResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryRedelegationsResponse as RawQueryRedelegationsResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse;

pub type QueryDelegationResponse = RawQueryDelegationResponse;
pub type QueryUnbondingDelegationResponse = RawQueryUnbondingDelegationResponse;
pub type QueryRedelegationsResponse = RawQueryRedelegationsResponse;
