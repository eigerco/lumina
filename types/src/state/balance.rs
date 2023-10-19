use celestia_proto::cosmos::base::v1beta1::Coin as RawCoin;
use serde::{Deserialize, Serialize};

use crate::state::Uint;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawCoin")]
pub struct Balance {
    pub denom: String,
    pub amount: Uint,
}

impl Balance {
    pub fn validate(&self) -> Result<()> {
        validate_denom(&self.denom)
    }
}

impl Serialize for Balance {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let raw: RawCoin = self
            .to_owned()
            .try_into()
            .map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
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

impl TryFrom<Balance> for RawCoin {
    type Error = Error;

    fn try_from(value: Balance) -> Result<Self, Self::Error> {
        value.validate()?;

        Ok(RawCoin {
            denom: value.denom,
            amount: value.amount.to_string(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn deserialize_balance() {
        let s = r#"{"denom":"abcd","amount":"1234"}"#;
        let balance: Balance = serde_json::from_str(s).unwrap();
        assert_eq!(balance.denom, "abcd");
        assert_eq!(balance.amount, Uint::from(1234));
    }

    #[test]
    fn deserialize_invalid_denom() {
        let s = r#"{"denom":"0asdadas","amount":"1234"}"#;
        serde_json::from_str::<Balance>(s).unwrap_err();
    }

    #[test]
    fn deserialize_invalid_amount() {
        let s = r#"{"denom":"abcd","amount":"a1234"}"#;
        serde_json::from_str::<Balance>(s).unwrap_err();
    }

    #[test]
    fn serialize_balance() {
        let balance = Balance {
            denom: "abcd".to_string(),
            amount: Uint::from(1234),
        };

        let s = serde_json::to_string(&balance).unwrap();
        let expected = r#"{"denom":"abcd","amount":"1234"}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn serialize_invalid_balance() {
        let balance = Balance {
            denom: "0sdfsfs".to_string(),
            amount: Uint::from(1234),
        };

        serde_json::to_string(&balance).unwrap_err();
    }

    #[test]
    fn valid_denom() {
        validate_denom("abc").unwrap();
        validate_denom("a01").unwrap();
        validate_denom("A01").unwrap();
        validate_denom("z01").unwrap();
        validate_denom("Z01").unwrap();
        validate_denom("aAzZ09/:._-").unwrap();
    }

    #[test]
    fn small_denom() {
        validate_denom("aa").unwrap_err();
        validate_denom("aaa").unwrap();
    }

    #[test]
    fn large_denom() {
        validate_denom(&"a".repeat(128)).unwrap();
        validate_denom(&"a".repeat(129)).unwrap_err();
    }

    #[test]
    fn demon_starting_with_number() {
        validate_denom("0bc").unwrap_err();
    }

    #[test]
    fn demon_starting_with_symbol() {
        validate_denom("_bc").unwrap_err();
    }

    #[test]
    fn demon_invalid_symbol() {
        validate_denom("abc$").unwrap_err();
    }
}
