//! types related to accounts

use celestia_proto::cosmos::auth::v1beta1::BaseAccount as RawBaseAccount;
use celestia_proto::cosmos::auth::v1beta1::ModuleAccount as RawModuleAccount;
use celestia_tendermint::public_key::PublicKey;
use celestia_tendermint_proto::Protobuf;

use crate::Error;

/// Params defines the parameters for the auth module.
#[derive(Debug)]
pub struct AuthParams {
    /// Maximum number of memo characters
    pub max_memo_characters: u64,
    /// Maximum nubmer of signatures
    pub tx_sig_limit: u64,
    /// Cost per transaction byte
    pub tx_size_cost_per_byte: u64,
    /// Cost to verify ed25519 signature
    pub sig_verify_cost_ed25519: u64,
    /// Cost to verify secp255k1 signature
    pub sig_verify_cost_secp256k1: u64,
}

/// [`BaseAccount`] defines a base account type.
///
/// It contains all the necessary fields for basic account functionality.
///
/// Any custom account type should extend this type for additional functionality
/// (e.g. vesting).
#[derive(Debug, Clone, PartialEq)]
pub struct BaseAccount {
    /// Bech32 [`AccountId`] of this account.
    pub address: String,
    /// Optional [`PublicKey`] associated with this account.
    pub pub_key: Option<PublicKey>,
    /// `account_number` is the account number of the account in state
    pub account_number: u64,
    /// Sequence of the account, which describes the number of committed transactions signed by a
    /// given address.
    pub sequence: u64,
}

/// ModuleAccount defines an account for modules that holds coins on a pool.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleAccount {
    /// [`BaseAccount`] specification of this module account.
    pub base_account: Option<BaseAccount>,
    /// Name of the module.
    pub name: String,
    /// Permissions associated with this module account.
    pub permissions: Vec<String>,
}

impl From<BaseAccount> for RawBaseAccount {
    fn from(account: BaseAccount) -> Self {
        RawBaseAccount {
            address: account.address,
            pub_key: None, //todo!(),
            account_number: account.account_number,
            sequence: account.sequence,
        }
    }
}

impl TryFrom<RawBaseAccount> for BaseAccount {
    type Error = Error;

    fn try_from(account: RawBaseAccount) -> Result<Self, Self::Error> {
        Ok(BaseAccount {
            address: account.address,
            pub_key: None,
            account_number: account.account_number,
            sequence: account.sequence,
        })
    }
}

impl From<ModuleAccount> for RawModuleAccount {
    fn from(account: ModuleAccount) -> Self {
        let base_account = account.base_account.map(BaseAccount::into);
        RawModuleAccount {
            base_account,
            name: account.name,
            permissions: account.permissions,
        }
    }
}

impl TryFrom<RawModuleAccount> for ModuleAccount {
    type Error = Error;

    fn try_from(account: RawModuleAccount) -> Result<Self, Self::Error> {
        let base_account = account
            .base_account
            .map(RawBaseAccount::try_into)
            .transpose()?;
        Ok(ModuleAccount {
            base_account,
            name: account.name,
            permissions: account.permissions,
        })
    }
}

impl Protobuf<RawBaseAccount> for BaseAccount {}

impl Protobuf<RawModuleAccount> for ModuleAccount {}
