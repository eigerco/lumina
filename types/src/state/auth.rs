//! types related to accounts

use celestia_proto::cosmos::crypto::ed25519::PubKey as Ed25519PubKey;
use celestia_proto::cosmos::crypto::secp256k1::PubKey as Secp256k1PubKey;
use prost::Message;
use tendermint::public_key::PublicKey;
use tendermint_proto::Protobuf;
use tendermint_proto::google::protobuf::Any;

use crate::Error;
use crate::state::AccAddress;
use crate::validation_error;

pub use celestia_proto::cosmos::auth::v1beta1::BaseAccount as RawBaseAccount;
pub use celestia_proto::cosmos::auth::v1beta1::ModuleAccount as RawModuleAccount;
pub use celestia_proto::cosmos::auth::v1beta1::Params as AuthParams;

const COSMOS_ED25519_PUBKEY: &str = "/cosmos.crypto.ed25519.PubKey";
const COSMOS_SECP256K1_PUBKEY: &str = "/cosmos.crypto.secp256k1.PubKey";

// TODO: add vesting accounts
/// Enum representing different types of account
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Account {
    /// Base account type
    Base(BaseAccount),
    /// Account for modules that holds coins on a pool
    Module(ModuleAccount),
}

impl std::ops::Deref for Account {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        match self {
            Account::Base(base) => base,
            Account::Module(module) => &module.base_account,
        }
    }
}

impl std::ops::DerefMut for Account {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Account::Base(base) => base,
            Account::Module(module) => &mut module.base_account,
        }
    }
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
    pub address: AccAddress,
    /// Optional `PublicKey` associated with this account.
    pub pub_key: Option<PublicKey>,
    /// `account_number` is the account number of the account in state
    pub account_number: u64,
    /// Sequence of the account, which describes the number of committed transactions signed by a
    /// given address.
    pub sequence: u64,
}

/// [`ModuleAccount`] defines an account for modules that holds coins on a pool.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ModuleAccount {
    /// [`BaseAccount`] specification of this module account.
    pub base_account: BaseAccount,
    /// Name of the module.
    pub name: String,
    /// Permissions associated with this module account.
    pub permissions: Vec<String>,
}

impl From<BaseAccount> for RawBaseAccount {
    fn from(account: BaseAccount) -> Self {
        RawBaseAccount {
            address: account.address.to_string(),
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
            address: account.address.parse()?,
            pub_key,
            account_number: account.account_number,
            sequence: account.sequence,
        })
    }
}

impl From<ModuleAccount> for RawModuleAccount {
    fn from(account: ModuleAccount) -> Self {
        let base_account = Some(account.base_account.into());
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
            .ok_or_else(|| validation_error!("base account missing"))?
            .try_into()?;
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
            .encode_to_vec(),
        },
        key @ PublicKey::Secp256k1(_) => Any {
            type_url: COSMOS_SECP256K1_PUBKEY.to_string(),
            value: Secp256k1PubKey {
                key: key.to_bytes(),
            }
            .encode_to_vec(),
        },
        _ => unimplemented!("unexpected key type"),
    }
}

impl Protobuf<RawBaseAccount> for BaseAccount {}

impl Protobuf<RawModuleAccount> for ModuleAccount {}

#[cfg(feature = "uniffi")]
mod uniffi_types {
    use std::str::FromStr;

    use super::{BaseAccount as RustBaseAccount, PublicKey as TendermintPublicKey};

    use tendermint::public_key::{Ed25519, Secp256k1};
    use uniffi::{Enum, Record};

    use crate::{error::UniffiConversionError, state::Address};

    #[derive(Enum)]
    pub enum PublicKey {
        Ed25519 { bytes: Vec<u8> },
        Secp256k1 { sec1_bytes: Vec<u8> },
    }

    impl TryFrom<PublicKey> for TendermintPublicKey {
        type Error = UniffiConversionError;

        fn try_from(value: PublicKey) -> Result<Self, Self::Error> {
            Ok(match value {
                PublicKey::Ed25519 { bytes } => TendermintPublicKey::Ed25519(
                    Ed25519::try_from(bytes.as_ref())
                        .map_err(|_| UniffiConversionError::InvalidPublicKey)?,
                ),
                PublicKey::Secp256k1 { sec1_bytes } => TendermintPublicKey::Secp256k1(
                    Secp256k1::from_sec1_bytes(&sec1_bytes)
                        .map_err(|_| UniffiConversionError::InvalidPublicKey)?,
                ),
            })
        }
    }

    impl From<TendermintPublicKey> for PublicKey {
        fn from(value: TendermintPublicKey) -> Self {
            match value {
                TendermintPublicKey::Ed25519(k) => PublicKey::Ed25519 {
                    bytes: k.as_bytes().to_vec(),
                },
                TendermintPublicKey::Secp256k1(k) => PublicKey::Secp256k1 {
                    sec1_bytes: k.to_sec1_bytes().to_vec(),
                },
                _ => unimplemented!("unexpected key type"),
            }
        }
    }

    uniffi::custom_type!(TendermintPublicKey, PublicKey, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });

    #[derive(Record)]
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

    impl TryFrom<BaseAccount> for RustBaseAccount {
        type Error = UniffiConversionError;

        fn try_from(value: BaseAccount) -> Result<Self, Self::Error> {
            let address = Address::from_str(&value.address)
                .map_err(|e| UniffiConversionError::InvalidAddress { msg: e.to_string() })?;
            let Address::AccAddress(account_address) = address else {
                return Err(UniffiConversionError::InvalidAddress {
                    msg: "Invalid address type, expected account address".to_string(),
                });
            };

            Ok(RustBaseAccount {
                address: account_address,
                pub_key: value.pub_key.map(TryInto::try_into).transpose()?,
                account_number: value.account_number,
                sequence: value.sequence,
            })
        }
    }

    impl From<RustBaseAccount> for BaseAccount {
        fn from(value: RustBaseAccount) -> Self {
            BaseAccount {
                address: value.address.to_string(),
                pub_key: value.pub_key.map(Into::into),
                account_number: value.account_number,
                sequence: value.sequence,
            }
        }
    }

    uniffi::custom_type!(RustBaseAccount, BaseAccount);
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use js_sys::{BigInt, Uint8Array};
    use lumina_utils::make_object;
    use tendermint::PublicKey;
    use wasm_bindgen::{JsCast, prelude::*};

    use super::{Account, AuthParams};

    #[wasm_bindgen(typescript_custom_section)]
    const _: &str = r#"
    /**
     * Public key
     */
    export interface PublicKey {
      type: "ed25519" | "secp256k1",
      value: Uint8Array
    }

    /**
     * Common data of all account types
     */
    export interface BaseAccount {
      address: string,
      pubkey?: PublicKey,
      accountNumber: bigint,
      sequence: bigint
    }

    /**
     * Auth module parameters
     */
    export interface AuthParams {
      maxMemoCharacters: bigint,
      txSigLimit: bigint,
      txSizeCostPerByte: bigint,
      sigVerifyCostEd25519: bigint,
      sigVerifyCostSecp256k1: bigint
    }
    "#;

    #[wasm_bindgen]
    extern "C" {
        /// Public key exposed to javascript.
        #[derive(Clone, Debug)]
        #[wasm_bindgen(typescript_type = "PublicKey")]
        pub type JsPublicKey;

        /// AuthParams exposed to JS
        #[wasm_bindgen(typescript_type = "AuthParams")]
        pub type JsAuthParams;

        /// BaseAccount exposed to javascript.
        #[wasm_bindgen(typescript_type = "BaseAccount")]
        pub type JsBaseAccount;
    }

    impl From<AuthParams> for JsAuthParams {
        fn from(value: AuthParams) -> JsAuthParams {
            let obj = make_object!(
                "maxMemoCharacters" => BigInt::from(value.max_memo_characters),
                "txSigLimit" => BigInt::from(value.tx_sig_limit),
                "txSizeCostPerByte" => BigInt::from(value.tx_size_cost_per_byte),
                "sigVerifyCostEd25519" => BigInt::from(value.sig_verify_cost_ed25519),
                "sigVerifyCostSecp256k1" => BigInt::from(value.sig_verify_cost_secp256k1)
            );

            obj.unchecked_into()
        }
    }

    impl From<PublicKey> for JsPublicKey {
        fn from(value: PublicKey) -> JsPublicKey {
            let algo = match value {
                PublicKey::Ed25519(..) => "ed25519",
                PublicKey::Secp256k1(..) => "secp256k1",
                _ => unreachable!("unsupported pubkey algo found"),
            };
            let obj = make_object!(
                "type" => algo.into(),
                "value" => Uint8Array::from(value.to_bytes().as_ref())
            );

            obj.unchecked_into()
        }
    }

    impl From<Account> for JsBaseAccount {
        fn from(value: Account) -> JsBaseAccount {
            let obj = make_object!(
                "address" => value.address.to_string().into(),
                "pubkey" => value.pub_key.map(JsPublicKey::from).into(),
                "accountNumber" => BigInt::from(value.account_number),
                "sequence" => BigInt::from(value.sequence)
            );

            obj.unchecked_into()
        }
    }
}
