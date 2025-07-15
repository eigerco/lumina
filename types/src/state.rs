//! Types and interfaces for accessing Celestia's state-relevant information.

mod address;
pub mod auth;
mod bit_array;
mod coin;
mod query_delegation;
mod tx;

#[cfg(feature = "uniffi")]
pub use self::address::uniffi_types::AccountId as UniffiAccountId;
pub use self::address::{AccAddress, Address, AddressKind, AddressTrait, ConsAddress, ValAddress};
pub use self::coin::Coin;
pub use self::query_delegation::{
    Delegation, DelegationResponse, PageRequest, PageResponse, QueryDelegationResponse,
    QueryRedelegationsResponse, QueryUnbondingDelegationResponse, Redelegation, RedelegationEntry,
    RedelegationEntryResponse, RedelegationResponse, UnbondingDelegation, UnbondingDelegationEntry,
};
pub use self::tx::{
    AuthInfo, ErrorCode, Fee, ModeInfo, RawTx, RawTxBody, RawTxResponse, SignerInfo, Sum, Tx,
    TxBody, TxResponse, BOND_DENOM,
};
