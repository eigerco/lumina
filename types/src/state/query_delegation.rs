use celestia_proto::cosmos::base::query::v1beta1::{
    PageRequest as RawPageRequest, PageResponse as RawPageResponse,
};
use celestia_proto::cosmos::staking::v1beta1::{
    Delegation as RawDelegation, DelegationResponse as RawDelegationResponse,
    QueryDelegationResponse as RawQueryDelegationResponse,
    QueryRedelegationsResponse as RawQueryRedelegationsResponse,
    QueryUnbondingDelegationResponse as RawQueryUnbondingDelegationResponse,
    Redelegation as RawRedelegation, RedelegationEntry as RawRedelegationEntry,
    RedelegationEntryResponse as RawRedelegationEntryResponse,
    RedelegationResponse as RawRedelegationResponse, UnbondingDelegation as RawUnbondingDelegation,
    UnbondingDelegationEntry as RawUnbondingDelegationEntry,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tendermint::Time;

use crate::error::{Error, Result};
use crate::state::{AccAddress, Coin, ValAddress};
use crate::Height;

/// Pagination details for the request.
pub type PageRequest = RawPageRequest;
/// Pagination details of the response.
pub type PageResponse = RawPageResponse;

/// Response type for the `QueryDelegation` RPC method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    try_from = "RawQueryDelegationResponse",
    into = "RawQueryDelegationResponse"
)]
pub struct QueryDelegationResponse {
    /// Delegation details including shares and current balance.
    pub response: DelegationResponse,
}

/// Contains a delegation and its corresponding token balance.
///
/// Used in client responses to show both the delegation data and the current
/// token amount derived from shares using the validator's exchange rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl From<QueryDelegationResponse> for RawQueryDelegationResponse {
    fn from(value: QueryDelegationResponse) -> Self {
        let balance = Coin::utia(value.response.balance);
        let delegation = value.response.delegation;
        RawQueryDelegationResponse {
            delegation_response: Some(RawDelegationResponse {
                delegation: Some(RawDelegation {
                    delegator_address: delegation.delegator_address.to_string(),
                    validator_address: delegation.validator_address.to_string(),
                    shares: cosmos_dec_to_string(&delegation.shares),
                }),
                balance: Some(balance.into()),
            }),
        }
    }
}

/// Response type for the `QueryUnbondingDelegation` RPC method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    try_from = "RawQueryUnbondingDelegationResponse",
    into = "RawQueryUnbondingDelegationResponse"
)]
pub struct QueryUnbondingDelegationResponse {
    /// Unbonding data for the delegator–validator pair.
    pub unbond: UnbondingDelegation,
}

/// Represents all unbonding entries between a delegator and a validator, ordered by time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingDelegation {
    /// Address of the delegator.
    pub delegator_address: AccAddress,
    /// Address of the validator.
    pub validator_address: ValAddress,
    /// List of unbonding entries, ordered by completion time.
    pub entries: Vec<UnbondingDelegationEntry>,
}

/// Represents a single unbonding entry with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingDelegationEntry {
    /// Block height at which unbonding began.
    pub creation_height: Height,
    /// Time when the unbonding will complete.
    pub completion_time: Time,
    /// Amount of tokens originally scheduled for unbonding.
    pub initial_balance: u64,
    /// Remaining tokens to be released at completion.
    pub balance: u64,
    /// Incrementing id that uniquely identifies this entry
    pub unbonding_id: u64,
    /// Strictly positive if this entry's unbonding has been stopped by external modules
    pub unbonding_on_hold_ref_count: i64,
}

impl From<QueryUnbondingDelegationResponse> for RawQueryUnbondingDelegationResponse {
    fn from(value: QueryUnbondingDelegationResponse) -> Self {
        RawQueryUnbondingDelegationResponse {
            unbond: Some(RawUnbondingDelegation {
                delegator_address: value.unbond.delegator_address.to_string(),
                validator_address: value.unbond.validator_address.to_string(),
                entries: value.unbond.entries.into_iter().map(Into::into).collect(),
            }),
        }
    }
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
                    unbonding_id: entry.unbonding_id,
                    unbonding_on_hold_ref_count: entry.unbonding_on_hold_ref_count,
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

impl From<UnbondingDelegationEntry> for RawUnbondingDelegationEntry {
    fn from(value: UnbondingDelegationEntry) -> Self {
        RawUnbondingDelegationEntry {
            creation_height: value.creation_height.into(),
            completion_time: Some(value.completion_time.into()),
            initial_balance: value.initial_balance.to_string(),
            balance: value.balance.to_string(),
            unbonding_id: value.unbonding_id,
            unbonding_on_hold_ref_count: value.unbonding_on_hold_ref_count,
        }
    }
}

/// Response type for the `QueryRedelegations` RPC method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    try_from = "RawQueryRedelegationsResponse",
    into = "RawQueryRedelegationsResponse"
)]
pub struct QueryRedelegationsResponse {
    /// List of redelegation responses, one per delegator–validator pair.
    pub responses: Vec<RedelegationResponse>,
    /// Pagination details of the response.
    pub pagination: Option<PageResponse>,
}

/// A redelegation record formatted for client responses.
///
/// Similar to [`Redelegation`], but each entry in `entries` includes a token balance
/// in addition to the original redelegation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedelegationEntryResponse {
    /// Original redelegation entry.
    pub redelegation_entry: RedelegationEntry,
    /// Token amount represented by this redelegation entry.
    pub balance: u64,
}

/// Represents a single redelegation entry with related metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedelegationEntry {
    /// Block height at which redelegation began.
    pub creation_height: Height,
    /// Time when the redelegation will complete.
    pub completion_time: Time,
    /// Token amount at the start of redelegation.
    pub initial_balance: u64,
    /// Amount of shares created in the destination validator.
    pub dest_shares: Decimal,
    /// Incrementing id that uniquely identifies this entry
    pub unbonding_id: u64,
    /// Strictly positive if this entry's unbonding has been stopped by external modules
    pub unbonding_on_hold_ref_count: i64,
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

impl From<RedelegationEntry> for RawRedelegationEntry {
    fn from(value: RedelegationEntry) -> Self {
        RawRedelegationEntry {
            creation_height: value.creation_height.into(),
            completion_time: Some(value.completion_time.into()),
            initial_balance: value.initial_balance.to_string(),
            shares_dst: cosmos_dec_to_string(&value.dest_shares),
            unbonding_id: value.unbonding_id,
            unbonding_on_hold_ref_count: value.unbonding_on_hold_ref_count,
        }
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
            unbonding_id: value.unbonding_id,
            unbonding_on_hold_ref_count: value.unbonding_on_hold_ref_count,
        })
    }
}

impl From<QueryRedelegationsResponse> for RawQueryRedelegationsResponse {
    fn from(value: QueryRedelegationsResponse) -> Self {
        RawQueryRedelegationsResponse {
            redelegation_responses: value
                .responses
                .into_iter()
                .map(|response| RawRedelegationResponse {
                    redelegation: Some(RawRedelegation {
                        delegator_address: response.redelegation.delegator_address.to_string(),
                        validator_src_address: response
                            .redelegation
                            .src_validator_address
                            .to_string(),
                        validator_dst_address: response
                            .redelegation
                            .dest_validator_address
                            .to_string(),
                        entries: response
                            .redelegation
                            .entries
                            .into_iter()
                            .map(Into::into)
                            .collect(),
                    }),
                    entries: response
                        .entries
                        .into_iter()
                        .map(|entry| RawRedelegationEntryResponse {
                            redelegation_entry: Some(entry.redelegation_entry.into()),
                            balance: entry.balance.to_string(),
                        })
                        .collect(),
                })
                .collect(),
            pagination: value.pagination,
        }
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
            responses: redelegation_responses,
            pagination: value.pagination,
        })
    }
}

const FIXED_POINT_EXPONENT: u32 = 18;

/// Parse Cosmos decimal
///
/// A Cosmos decimal is serialized as string and has 18 decimal places.
///
/// Ref: https://github.com/celestiaorg/cosmos-sdk/blob/5259747ebf054c2148032202d946945a2d1896c7/math/dec.go#L21
fn parse_cosmos_dec(s: &str) -> Result<Decimal> {
    let val = s
        .parse::<i128>()
        .map_err(|_| Error::InvalidCosmosDecimal(s.to_owned()))?;
    let val = Decimal::try_from_i128_with_scale(val, FIXED_POINT_EXPONENT)
        .map_err(|_| Error::InvalidCosmosDecimal(s.to_owned()))?;
    Ok(val)
}

/// Encode Cosmos decimal. Should be inverse of [`parse_cosmos_dec`]
fn cosmos_dec_to_string(d: &Decimal) -> String {
    let mantissa = d.mantissa();
    debug_assert_eq!(d.scale(), FIXED_POINT_EXPONENT);
    format!("{mantissa}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_decimal_round_trip() {
        let epsilon = "1";
        let v = parse_cosmos_dec(epsilon).unwrap();
        assert_eq!(cosmos_dec_to_string(&v), epsilon);
        assert_eq!(v, Decimal::new(1, FIXED_POINT_EXPONENT));

        let unit = "1000000000000000000";
        let v = parse_cosmos_dec(unit).unwrap();
        assert_eq!(cosmos_dec_to_string(&v), unit);
        assert_eq!(u32::try_from(v).unwrap(), 1);
    }
}
