use serde::{Deserialize, Serialize};

use celestia_types::hash::Hash;
use celestia_types::Height;

use crate::grpc::TxPriority;

pub use celestia_proto::cosmos::tx::v1beta1::SignDoc;

/// A result of correctly submitted transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TxInfo {
    /// Hash of the transaction.
    pub hash: Hash,
    /// Height at which transaction was submitted.
    pub height: Height,
}

/// Configuration for the transaction.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TxConfig {
    /// Custom gas limit for the transaction (in `utia`). By default, client will
    /// query gas estimation service to get estimate gas limit.
    pub gas_limit: Option<u64>,
    /// Custom gas price for fee calculation. By default, client will query gas
    /// estimation service to get gas price estimate.
    pub gas_price: Option<f64>,
    /// Memo for the transaction
    pub memo: Option<String>,
    /// Priority of the transaction, used with gas estimation service
    pub priority: TxPriority,
}

impl TxConfig {
    /// Attach gas limit to this config.
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Attach gas price to this config.
    pub fn with_gas_price(mut self, gas_price: f64) -> Self {
        self.gas_price = Some(gas_price);
        self
    }

    /// Attach memo to this config.
    pub fn with_memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }

    /// Specify transaction priority to be used when using gas estimation service
    pub fn with_priority(mut self, priority: TxPriority) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use super::{TxConfig, TxInfo, TxPriority};
    use lumina_utils::make_object;
    use wasm_bindgen::{prelude::*, JsCast};

    #[wasm_bindgen(typescript_custom_section)]
    const _: &str = "
    /**
     * Transaction info
     */
    export interface TxInfo {
      /**
       * Hash of the transaction.
       */
      hash: string;
      /**
       * Height at which transaction was submitted.
       */
      height: bigint;
    }

    /**
     * Transaction config.
     */
    export interface TxConfig {
      /**
       * Custom gas limit for the transaction (in `utia`). By default, client will
       * query gas estimation service to get estimate gas limit.
       */
      gasLimit?: bigint; // utia
      /**
       * Custom gas price for fee calculation. By default, client will query gas
       * estimation service to get gas price estimate.
       */
      gasPrice?: number;
      /**
       * Memo for the transaction
       */
      memo?: string;
      /**
       * Priority of the transaction, used with gas estimation service
       */
      priority?: TxPriority;
    }
    ";

    #[wasm_bindgen]
    extern "C" {
        /// TxInfo exposed to javascript
        #[wasm_bindgen(typescript_type = "TxInfo")]
        pub type JsTxInfo;

        /// TxConfig exposed to javascript
        #[wasm_bindgen(typescript_type = "TxConfig")]
        pub type JsTxConfig;

        #[wasm_bindgen(method, getter, js_name = gasLimit)]
        pub fn gas_limit(this: &JsTxConfig) -> Option<u64>;

        #[wasm_bindgen(method, getter, js_name = gasPrice)]
        pub fn gas_price(this: &JsTxConfig) -> Option<f64>;

        #[wasm_bindgen(method, getter, js_name = memo)]
        pub fn memo(this: &JsTxConfig) -> Option<String>;

        #[wasm_bindgen(method, getter, js_name = priority)]
        pub fn priority(this: &JsTxConfig) -> Option<TxPriority>;
    }

    impl From<TxInfo> for JsTxInfo {
        fn from(value: TxInfo) -> JsTxInfo {
            let obj = make_object!(
                "hash" => value.hash.to_string().into(),
                "height" => js_sys::BigInt::from(value.height.value())
            );

            obj.unchecked_into()
        }
    }

    impl From<JsTxConfig> for TxConfig {
        fn from(value: JsTxConfig) -> TxConfig {
            TxConfig {
                gas_limit: value.gas_limit(),
                gas_price: value.gas_price(),
                memo: value.memo(),
                priority: value.priority().unwrap_or_default(),
            }
        }
    }
}
