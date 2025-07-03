use celestia_proto::cosmos::staking::v1beta1::Delegation as RawDelegation;
use celestia_proto::cosmos::staking::v1beta1::QueryDelegationResponse as RawQueryDelegationResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryRedelegationsResponse as RawQueryRedelegationsResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::state::{AccAddress, Coin, ValAddress};

/// Status of the unbonding between a delegator and a validator.
pub type QueryUnbondingDelegationResponse = RawQueryUnbondingDelegationResponse;
/// Status of the redelegation between a delegator and a validator.
pub type QueryRedelegationsResponse = RawQueryRedelegationsResponse;

/// Status of the delegation between a delegator and a validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct QueryDelegationResponse {
    pub delegation: Delegation,
    pub balance: Coin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Delegation {
    pub delegator_address: AccAddress,
    pub validator_address: ValAddress,
    pub shares: String, // TODO
}

impl TryFrom<RawQueryDelegationResponse> for QueryDelegationResponse {
    type Error = Error;

    fn try_from(value: RawQueryDelegationResponse) -> Result<QueryDelegationResponse> {
        let resp = value
            .delegation_response
            .ok_or(Error::MissingDelegationResponse)?;

        let delegation = resp
            .delegation
            .ok_or(Error::MissingDelegation)?
            .try_into()?;

        let balance = resp.balance.ok_or(Error::MissingBalance)?.try_into()?;

        Ok(QueryDelegationResponse {
            delegation,
            balance,
        })
    }
}

impl TryFrom<RawDelegation> for Delegation {
    type Error = Error;

    fn try_from(value: RawDelegation) -> Result<Self> {
        Ok(Delegation {
            delegator_address: value.delegator_address.parse()?,
            validator_address: value.validator_address.parse()?,
            shares: value.shares,
        })
    }
}
