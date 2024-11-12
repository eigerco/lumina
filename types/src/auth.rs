//! types related to accounts

use celestia_proto::cosmos::auth::v1beta1::BaseAccount as RawBaseAccount;
use celestia_proto::cosmos::auth::v1beta1::ModuleAccount as RawModuleAccount;
use celestia_proto::cosmos::crypto::ed25519::PubKey as Ed25519PubKey;
use celestia_proto::cosmos::crypto::secp256k1::PubKey as Secp256k1PubKey;
use celestia_tendermint::public_key::PublicKey;
use celestia_tendermint_proto::Protobuf;

#[cfg(feature = "tonic")]
use pbjson_types::Any;
#[cfg(not(feature = "tonic"))]
use prost_types::Any;
use prost::Message;

use crate::Error;

const COSMOS_ED25519_PUBKEY: &str = "/cosmos.crypto.ed25519.PubKey";
const COSMOS_SECP256K1_PUBKEY: &str = "/cosmos.crypto.secp256k1.PubKey";

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
    /// Bech32 `AccountId` of this account.
    pub address: String,
    /// Optional `PublicKey` associated with this account.
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
            pub_key: account.pub_key.map(any_from_public_key),
            account_number: account.account_number,
            sequence: account.sequence,
        }
    }
}

impl TryFrom<RawBaseAccount> for BaseAccount {
    type Error = Error;

    fn try_from(account: RawBaseAccount) -> Result<Self, Self::Error> {
        let pub_key = account.pub_key.map(public_key_from_any).transpose()?;
        Ok(BaseAccount {
            address: account.address,
            pub_key,
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

fn public_key_from_any(any: Any) -> Result<PublicKey, Error> {
    match any.type_url.as_ref() {
        COSMOS_ED25519_PUBKEY => {
            PublicKey::from_raw_ed25519(&Ed25519PubKey::decode(&*any.value)?.key)
        }
        COSMOS_SECP256K1_PUBKEY => {
            PublicKey::from_raw_secp256k1(&Secp256k1PubKey::decode(&*any.value)?.key)
        }
        other => return Err(Error::InvalidPublicKeyType(other.to_string())),
    }
    .ok_or(Error::InvalidPublicKey)
}

fn any_from_public_key(key: PublicKey) -> Any {
    match key {
        key @ PublicKey::Ed25519(_) => Any {
            type_url: COSMOS_ED25519_PUBKEY.to_string(),
            value: Ed25519PubKey {
                key: key.to_bytes(),
            }
            .encode_to_vec()
            .into(),
        },
        key @ PublicKey::Secp256k1(_) => Any {
            type_url: COSMOS_SECP256K1_PUBKEY.to_string(),
            value: Secp256k1PubKey {
                key: key.to_bytes(),
            }
            .encode_to_vec()
            .into(),
        },
        _ => unimplemented!("unexpected key type"),
    }
}

impl Protobuf<RawBaseAccount> for BaseAccount {}

impl Protobuf<RawModuleAccount> for ModuleAccount {}
