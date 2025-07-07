use celestia_proto::cosmos::base::query::v1beta1::{
    PageRequest as RawPageRequest, PageResponse as RawPageResponse,
};
use celestia_proto::cosmos::staking::v1beta1::Delegation as RawDelegation;
use celestia_proto::cosmos::staking::v1beta1::QueryDelegationResponse as RawQueryDelegationResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryRedelegationsResponse as RawQueryRedelegationsResponse;
use celestia_proto::cosmos::staking::v1beta1::QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse;
use serde::{Deserialize, Serialize};
use tendermint::Time;

use crate::error::{Error, Result};
use crate::state::{AccAddress, Coin, ValAddress};
use crate::Height;

pub type PageRequest = RawPageRequest;
pub type PageResponse = RawPageResponse;

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
            .ok_or(Error::MissingDelegation)
            .and_then(|val| {
                Ok(Delegation {
                    delegator_address: val.delegator_address.parse()?,
                    validator_address: val.validator_address.parse()?,
                    shares: val.shares,
                })
            })?;

        let balance = resp.balance.ok_or(Error::MissingBalance)?.try_into()?;

        Ok(QueryDelegationResponse {
            delegation,
            balance,
        })
    }
}

/// Status of the unbonding between a delegator and a validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct QueryUnbondingDelegationResponse {
    pub unbond: UnbondingDelegation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UnbondingDelegation {
    pub delegator_address: AccAddress,
    pub validator_address: ValAddress,
    pub entries: Vec<UnbondingDelegationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UnbondingDelegationEntry {
    pub creation_height: Height,
    pub completion_time: Option<Time>,
    pub initial_balance: u64,
    pub balance: u64,
}

impl TryFrom<RawQueryUnbondingDelegationResponse> for QueryUnbondingDelegationResponse {
    type Error = Error;

    fn try_from(value: RawQueryUnbondingDelegationResponse) -> Result<Self, Self::Error> {
        let unbond = value.unbond.ok_or(Error::MissingUnbond)?;

        let delegator_address = unbond.delegator_address.parse()?;
        let validator_address = unbond.validator_address.parse()?;

        let entries = unbond
            .entries
            .into_iter()
            .map(|entry| {
                let creation_height = entry.creation_height.try_into()?;

                let completion_time = entry
                    .completion_time
                    .map(|time| time.try_into())
                    .transpose()?;

                let initial_balance = entry
                    .initial_balance
                    .parse()
                    .map_err(|_| Error::InvalidBalance(entry.initial_balance))?;

                let balance = entry
                    .balance
                    .parse()
                    .map_err(|_| Error::InvalidBalance(entry.balance))?;

                Ok(UnbondingDelegationEntry {
                    creation_height,
                    completion_time,
                    initial_balance,
                    balance,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        Ok(QueryUnbondingDelegationResponse {
            unbond: UnbondingDelegation {
                delegator_address,
                validator_address,
                entries,
            },
        })
    }
}

/// Status of the redelegation between a delegator and a validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct QueryRedelegationsResponse {
    pub redelegation_responses: Vec<RedelegationResponse>,
    pub pagination: Option<PageResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RedelegationResponse {
    pub redelegation: Redelegation,
    pub entries: Vec<RedelegationEntryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Redelegation {
    pub delegator_address: AccAddress,
    pub src_validator_address: ValAddress,
    pub dest_validator_address: ValAddress,
    pub entries: Vec<RedelegationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RedelegationEntryResponse {
    pub redelegation_entry: RedelegationEntry,
    pub balance: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RedelegationEntry {
    pub creation_height: Height,
    pub completion_time: Option<Time>,
    pub initial_balance: u64,
    pub shares_dst: String, // TODO
}

impl TryFrom<RawQueryRedelegationsResponse> for QueryRedelegationsResponse {
    type Error = Error;

    fn try_from(value: RawQueryRedelegationsResponse) -> std::result::Result<Self, Self::Error> {
        todo!();
    }
}
