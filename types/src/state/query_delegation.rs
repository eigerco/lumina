use celestia_proto::cosmos::staking::v1beta1::QueryDelegationResponse as RawQueryDelegationResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryRedelegationsResponse as RawQueryRedelegationsResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse;

/// Status of the delegation between a delegator and a validator.
pub type QueryDelegationResponse = RawQueryDelegationResponse;
/// Status of the unbonding between a delegator and a validator.
pub type QueryUnbondingDelegationResponse = RawQueryUnbondingDelegationResponse;
/// Status of the redelegation between a delegator and a validator.
pub type QueryRedelegationsResponse = RawQueryRedelegationsResponse;
