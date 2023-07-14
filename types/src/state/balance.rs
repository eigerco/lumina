use celestia_proto::cosmos::base::v1beta1::Coin as RawCoin;
use serde::{Deserialize, Serialize};

use crate::state::Uint;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawCoin", into = "RawCoin")]
pub struct Balance {
    pub denom: String,
    pub amount: Uint,
}

impl Balance {
    pub fn validate(&self) -> Result<()> {
        validate_denom(&self.denom)
    }
}

impl TryFrom<RawCoin> for Balance {
    type Error = Error;

    fn try_from(value: RawCoin) -> Result<Self, Self::Error> {
        validate_denom(&value.denom)?;

        let amount = value
            .amount
            .parse()
            .map_err(|_| Error::InvalidBalanceAmount(value.amount))?;

        Ok(Balance {
            denom: value.denom,
            amount,
        })
    }
}

impl From<Balance> for RawCoin {
    fn from(value: Balance) -> Self {
        RawCoin {
            denom: value.denom,
            amount: value.amount.to_string(),
        }
    }
}

fn validate_denom(denom: &str) -> Result<()> {
    // Length must be 3-128 characters
    if denom.len() < 3 || denom.len() > 128 {
        return Err(Error::InvalidBalanceDenomination(denom.to_owned()));
    }

    let mut chars = denom.chars();

    // First character must be a letter
    if !matches!(chars.next(), Some('a'..='z' | 'A'..='Z')) {
        return Err(Error::InvalidBalanceDenomination(denom.to_owned()));
    }

    // The rest can be a letter, a number, or a symbol from '/', ':', '.', '_', '-'
    if chars.all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '_' | '-')) {
        Ok(())
    } else {
        Err(Error::InvalidBalanceDenomination(denom.to_owned()))
    }
}
