//! Types and interfaces for accessing Celestia's state-relevant information.

mod address;
pub mod auth;
mod balance;
mod bit_array;
mod query_delegation;
mod tx;

#[cfg(feature = "uniffi")]
pub use self::address::uniffi_types::AccountId as UniffiAccountId;
pub use self::address::{
    AccAddress, Address, AddressKind, AddressTrait, Bech32Address, ConsAddress, ValAddress,
};
pub use self::balance::Balance;
pub use self::query_delegation::{
    QueryDelegationResponse, QueryRedelegationsResponse, QueryUnbondingDelegationResponse,
};
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use self::tx::JsCoin;
pub use self::tx::{
    AuthInfo, Coin, ErrorCode, Fee, ModeInfo, RawTx, RawTxBody, RawTxResponse, SignerInfo, Sum, Tx,
    TxBody, TxResponse, BOND_DENOM,
};

/// A 256-bit unsigned integer.
pub type Uint = ruint::aliases::U256;
