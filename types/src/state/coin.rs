use celestia_proto::cosmos::base::v1beta1::Coin as RawCoin;
use serde::{Deserialize, Serialize};
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
use wasm_bindgen::prelude::*;

use crate::{Error, Result};

/// Coin defines a token with a denomination and an amount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawCoin", into = "RawCoin")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
    wasm_bindgen(getter_with_clone)
)]
pub struct Coin {
    /// Coin denomination
    denom: String,
    /// Coin amount
    amount: u64,
}

impl Coin {
    /// Create a new coin with geven amount and denomination.
    pub fn new(denom: &str, amount: u64) -> Result<Coin> {
        validate_denom(denom)?;

        Ok(Coin {
            denom: denom.to_owned(),
            amount,
        })
    }

    /// Create a coin with given amount of `utia`.
    pub fn utia(amount: u64) -> Self {
        Coin::new("utia", amount).expect("denom is always valid")
    }

    /// Amount getter.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Amount setter.
    pub fn set_amount(&mut self, amount: u64) {
        self.amount = amount;
    }

    /// Denomination getter.
    pub fn denom(&self) -> &str {
        self.denom.as_str()
    }

    /// Denomination setter.
    pub fn set_denom(&mut self, denom: &str) -> Result<()> {
        validate_denom(denom)?;
        self.denom = denom.to_owned();
        Ok(())
    }
}

impl TryFrom<RawCoin> for Coin {
    type Error = Error;

    fn try_from(value: RawCoin) -> Result<Self, Self::Error> {
        validate_denom(&value.denom)?;

        let amount = value
            .amount
            .parse()
            .map_err(|_| Error::InvalidCoinAmount(value.amount))?;

        Ok(Coin {
            denom: value.denom,
            amount,
        })
    }
}

impl From<Coin> for RawCoin {
    fn from(value: Coin) -> Self {
        RawCoin {
            denom: value.denom,
            amount: value.amount.to_string(),
        }
    }
}

fn validate_denom(denom: &str) -> Result<()> {
    // Length must be 3-128 characters
    if denom.len() < 3 || denom.len() > 128 {
        return Err(Error::InvalidCoinDenomination(denom.to_owned()));
    }

    let mut chars = denom.chars();

    // First character must be a letter
    if !matches!(chars.next(), Some('a'..='z' | 'A'..='Z')) {
        return Err(Error::InvalidCoinDenomination(denom.to_owned()));
    }

    // The rest can be a letter, a number, or a symbol from '/', ':', '.', '_', '-'
    if chars.all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '_' | '-')) {
        Ok(())
    } else {
        Err(Error::InvalidCoinDenomination(denom.to_owned()))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use super::Coin;
    use js_sys::BigInt;
    use wasm_bindgen::prelude::*;

    use lumina_utils::make_object;

    #[wasm_bindgen(typescript_custom_section)]
    const _: &str = "
    /**
     * Coin
     */
    export interface Coin {
      denom: string,
      amount: bigint
    }
    ";

    #[wasm_bindgen]
    extern "C" {
        /// Coin exposed to javascript
        #[wasm_bindgen(typescript_type = "Coin")]
        pub type JsCoin;
    }

    impl From<Coin> for JsCoin {
        fn from(value: Coin) -> JsCoin {
            let obj = make_object!(
                "denom" => value.denom().into(),
                "amount" => BigInt::from(value.amount())
            );

            obj.unchecked_into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn deserialize_coin() {
        let s = r#"{"denom":"abcd","amount":"1234"}"#;
        let coin: Coin = serde_json::from_str(s).unwrap();
        assert_eq!(coin.denom(), "abcd");
        assert_eq!(coin.amount(), 1234);
    }

    #[test]
    fn deserialize_invalid_denom() {
        let s = r#"{"denom":"0asdadas","amount":"1234"}"#;
        serde_json::from_str::<Coin>(s).unwrap_err();
    }

    #[test]
    fn deserialize_invalid_amount() {
        let s = r#"{"denom":"abcd","amount":"a1234"}"#;
        serde_json::from_str::<Coin>(s).unwrap_err();
    }

    #[test]
    fn serialize_coin() {
        let coin = Coin::new("abcd", 1234).unwrap();
        let s = serde_json::to_string(&coin).unwrap();
        let expected = r#"{"denom":"abcd","amount":"1234"}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn invalid_coin_denom() {
        Coin::new("0bc", 1234).unwrap_err();

        let mut coin = Coin::utia(1);
        coin.set_denom("0bc").unwrap_err();
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
