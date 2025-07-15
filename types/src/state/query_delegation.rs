use celestia_proto::cosmos::base::query::v1beta1::{
    PageRequest as RawPageRequest, PageResponse as RawPageResponse,
};
use celestia_proto::cosmos::staking::v1beta1::{
    QueryDelegationResponse as RawQueryDelegationResponse,
    QueryRedelegationsResponse as RawQueryRedelegationsResponse,
    QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse,
    Redelegation as RawRedelegation, RedelegationEntry as RawRedelegationEntry,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use tendermint::Time;

use crate::error::{Error, Result};
use crate::state::{AccAddress, Coin, ValAddress};
use crate::Height;

pub type PageRequest = RawPageRequest;
pub type PageResponse = RawPageResponse;

/// Response type for the `QueryDelegation` RPC method.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawQueryDelegationResponse")]
pub struct QueryDelegationResponse {
    /// Delegation details including shares and current balance.
    pub response: DelegationResponse,
}

/// Contains a delegation and its corresponding token balance.
///
/// Used in client responses to show both the delegation data and the current
/// token amount derived from shares using the validator's exchange rate.
#[derive(Debug, Clone)]
pub struct DelegationResponse {
    /// Delegation data.
    pub delegation: Delegation,
    /// Token amount currently represented by the delegation shares.
    pub balance: u64,
}

/// Represents a bond with tokens held by an account.
///
/// A delegation is owned by one delegator and is associated with the voting power
/// of a single validator. It encapsulates the relationship and stake details between
/// a delegator and a validator.
#[derive(Debug, Clone)]
pub struct Delegation {
    /// Address of the delegator.
    pub delegator_address: AccAddress,
    /// Address of the validator.
    pub validator_address: ValAddress,
    /// Amount of delegation shares received.
    pub shares: Decimal,
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
                    shares: parse_cosmos_dec(&val.shares)?,
                })
            })?;

        let balance: Coin = resp.balance.ok_or(Error::MissingBalance)?.try_into()?;

        Ok(QueryDelegationResponse {
            response: DelegationResponse {
                delegation,
                balance: balance.amount(),
            },
        })
    }
}

/// Response type for the `QueryUnbondingDelegation` RPC method.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawQueryUnbondingDelegationResponse")]
pub struct QueryUnbondingDelegationResponse {
    /// Unbonding data for the delegator–validator pair.
    pub unbond: UnbondingDelegation,
}

/// Represents all unbonding entries between a delegator and a validator, ordered by time.
#[derive(Debug, Clone)]
pub struct UnbondingDelegation {
    /// Address of the delegator.
    pub delegator_address: AccAddress,
    /// Address of the validator.
    pub validator_address: ValAddress,
    /// List of unbonding entries, ordered by completion time.
    pub entries: Vec<UnbondingDelegationEntry>,
}

/// Represents a single unbonding entry with associated metadata.
#[derive(Debug, Clone)]
pub struct UnbondingDelegationEntry {
    /// Block height at which unbonding began.
    pub creation_height: Height,
    /// Time when the unbonding will complete.
    pub completion_time: Time,
    /// Amount of tokens originally scheduled for unbonding.
    pub initial_balance: u64,
    /// Remaining tokens to be released at completion.
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
                    .ok_or(Error::MissingCompletionTime)?
                    .try_into()?;

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

/// Response type for the `QueryRedelegations` RPC method.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawQueryRedelegationsResponse")]
pub struct QueryRedelegationsResponse {
    /// List of redelegation responses, one per delegator–validator pair.
    pub redelegation_responses: Vec<RedelegationResponse>,
    /// Pagination details for the response.
    pub pagination: Option<PageResponse>,
}

/// A redelegation record formatted for client responses.
///
/// Similar to [`Redelegation`], but each entry in `entries` includes a token balance
/// in addition to the original redelegation metadata.
#[derive(Debug, Clone)]
pub struct RedelegationResponse {
    /// Redelegation metadata (delegator, source, and destination validators).
    pub redelegation: Redelegation,

    /// Redelegation entries with token balances.
    ///
    /// Mirrors `redelegation.entries`, but each entry includes a `balance`
    /// field representing the current token amount.
    pub entries: Vec<RedelegationEntryResponse>,
}

/// Represents redelegation activity from one validator to another for a given delegator.
///
/// Contains all redelegation entries between a specific source and destination validator.
#[derive(Debug, Clone)]
pub struct Redelegation {
    /// Address of the delegator.
    pub delegator_address: AccAddress,
    /// Address of the source validator.
    pub src_validator_address: ValAddress,
    /// Address of the destination validator.
    pub dest_validator_address: ValAddress,
    /// List of individual redelegation entries.
    pub entries: Vec<RedelegationEntry>,
}

/// A redelegation entry along with the token balance it represents.
///
/// Used in client responses to include both the redelegation metadata and
/// the token amount associated with the destination validator.
#[derive(Debug, Clone)]
pub struct RedelegationEntryResponse {
    /// Original redelegation entry.
    pub redelegation_entry: RedelegationEntry,
    /// Token amount represented by this redelegation entry.
    pub balance: u64,
}

/// Represents a single redelegation entry with related metadata.
#[derive(Debug, Clone)]
pub struct RedelegationEntry {
    /// Block height at which redelegation began.
    pub creation_height: Height,
    /// Time when the redelegation will complete.
    pub completion_time: Time,
    /// Token amount at the start of redelegation.
    pub initial_balance: u64,
    /// Amount of shares created in the destination validator.
    pub dest_shares: Decimal,
}

impl TryFrom<RawRedelegation> for Redelegation {
    type Error = Error;

    fn try_from(value: RawRedelegation) -> Result<Self, Self::Error> {
        let entries = value
            .entries
            .into_iter()
            .map(|entry| entry.try_into())
            .collect::<Result<Vec<_>>>()?;

        Ok(Redelegation {
            delegator_address: value.delegator_address.parse()?,
            src_validator_address: value.validator_src_address.parse()?,
            dest_validator_address: value.validator_dst_address.parse()?,
            entries,
        })
    }
}

impl TryFrom<RawRedelegationEntry> for RedelegationEntry {
    type Error = Error;

    fn try_from(value: RawRedelegationEntry) -> Result<Self, Self::Error> {
        let initial_balance = value
            .initial_balance
            .as_str()
            .parse::<u64>()
            .map_err(|_| Error::InvalidBalance(value.initial_balance))?;

        Ok(RedelegationEntry {
            creation_height: value.creation_height.try_into()?,
            completion_time: value
                .completion_time
                .ok_or(Error::MissingCompletionTime)?
                .try_into()?,
            initial_balance,
            dest_shares: parse_cosmos_dec(&value.shares_dst)?,
        })
    }
}

impl TryFrom<RawQueryRedelegationsResponse> for QueryRedelegationsResponse {
    type Error = Error;

    fn try_from(value: RawQueryRedelegationsResponse) -> Result<Self, Self::Error> {
        let redelegation_responses = value
            .redelegation_responses
            .into_iter()
            .map(|resp| {
                let redelegation = resp
                    .redelegation
                    .ok_or(Error::MissingRedelegation)?
                    .try_into()?;

                let entries = resp
                    .entries
                    .into_iter()
                    .map(|resp| {
                        let redelegation_entry = resp
                            .redelegation_entry
                            .ok_or(Error::MissingRedelegationEntry)?
                            .try_into()?;

                        let balance = resp
                            .balance
                            .as_str()
                            .parse::<u64>()
                            .map_err(|_| Error::InvalidBalance(resp.balance))?;

                        Ok(RedelegationEntryResponse {
                            redelegation_entry,
                            balance,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;

                Ok(RedelegationResponse {
                    redelegation,
                    entries,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(QueryRedelegationsResponse {
            redelegation_responses,
            pagination: value.pagination,
        })
    }
}

/// Parse Cosmos decimal
///
/// A Cosmos decimal is serialized as string and has 18 decimal places.
///
/// Ref: https://github.com/celestiaorg/cosmos-sdk/blob/5259747ebf054c2148032202d946945a2d1896c7/math/dec.go#L21
fn parse_cosmos_dec(s: &str) -> Result<Decimal> {
    let val = s
        .parse::<i128>()
        .map_err(|_| Error::InvalidCosmosDecimal(s.to_owned()))?;
    let val = Decimal::try_from_i128_with_scale(val, 18)
        .map_err(|_| Error::InvalidCosmosDecimal(s.to_owned()))?;
    Ok(val)
}
