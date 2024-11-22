//! Types and interfaces for accessing Celestia's state-relevant information.

mod address;
pub mod auth;
mod balance;
mod query_delegation;
mod tx;

pub use self::address::{AccAddress, Address, AddressKind, AddressTrait, ConsAddress, ValAddress};
pub use self::balance::Balance;
pub use self::query_delegation::{
    QueryDelegationResponse, QueryRedelegationsResponse, QueryUnbondingDelegationResponse,
};
pub use self::tx::{
    AuthInfo, Coin, Fee, ModeInfo, RawTx, RawTxBody, RawTxResponse, SignerInfo, Sum, Tx, TxBody,
    TxResponse, BOND_DENOM,
};

/// A 256-bit unsigned integer.
pub type Uint = ruint::aliases::U256;
