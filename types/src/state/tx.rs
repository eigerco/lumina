use celestia_tendermint_proto::Protobuf;
#[cfg(feature = "tonic")]
use pbjson_types::Any;
#[cfg(not(feature = "tonic"))]
use prost_types::Any;
use serde::{Deserialize, Serialize};

use celestia_proto::cosmos::base::abci::v1beta1::AbciMessageLog;
use celestia_tendermint_proto::v0_34::abci::Event;

use crate::bit_array::BitVector;
use crate::Error;
use crate::Height;

pub use celestia_proto::cosmos::base::abci::v1beta1::TxResponse as RawTxResponse;
pub use celestia_proto::cosmos::base::v1beta1::Coin as RawCoin;
pub use celestia_proto::cosmos::tx::v1beta1::mode_info::Sum as RawSum;
pub use celestia_proto::cosmos::tx::v1beta1::mode_info::{Multi, Single};
pub use celestia_proto::cosmos::tx::v1beta1::AuthInfo as RawAuthInfo;
pub use celestia_proto::cosmos::tx::v1beta1::Fee as RawFee;
pub use celestia_proto::cosmos::tx::v1beta1::ModeInfo as RawModeInfo;
pub use celestia_proto::cosmos::tx::v1beta1::SignerInfo as RawSignerInfo;
pub use celestia_proto::cosmos::tx::v1beta1::Tx as RawTx;
pub use celestia_proto::cosmos::tx::v1beta1::TxBody as RawTxBody;

pub type Signature = Vec<u8>;

// [`BOND_DENOM`] defines the native staking denomination
pub const BOND_DENOM: &str = "utia";

/// [`Tx`] is the standard type used for broadcasting transactions.
#[derive(Debug, Clone)]
pub struct Tx {
    /// Processable content of the transaction
    pub body: TxBody,

    /// Authorization related content of the transaction, specifically signers, signer modes
    /// and [`Fee`].
    pub auth_info: AuthInfo,

    /// List of signatures that matches the length and order of [`AuthInfo`]’s `signer_info`s to
    /// allow connecting signature meta information like public key and signing mode by position.
    ///
    /// Signatures are provided as raw bytes so as to support current and future signature types.
    /// [`AuthInfo`] should be introspected to determine the signature algorithm used.
    pub signatures: Vec<Signature>,
}

/// [`TxBody`] of a transaction that all signers sign over.
#[derive(Debug, Clone)]
pub struct TxBody {
    /// `messages` is a list of messages to be executed. The required signers of
    /// those messages define the number and order of elements in `AuthInfo`'s
    /// signer_infos and Tx's signatures. Each required signer address is added to
    /// the list only the first time it occurs.
    ///
    /// By convention, the first required signer (usually from the first message)
    /// is referred to as the primary signer and pays the fee for the whole
    /// transaction.
    pub messages: Vec<Any>,
    /// `memo` is any arbitrary memo to be added to the transaction.
    pub memo: String,
    /// `timeout` is the block height after which this transaction will not
    /// be processed by the chain
    pub timeout_height: Height,
    /// `extension_options` are arbitrary options that can be added by chains
    /// when the default options are not sufficient. If any of these are present
    /// and can't be handled, the transaction will be rejected
    pub extension_options: Vec<Any>,
    /// `extension_options` are arbitrary options that can be added by chains
    /// when the default options are not sufficient. If any of these are present
    /// and can't be handled, they will be ignored
    pub non_critical_extension_options: Vec<Any>,
}

/// Response to a tx query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse {
    /// The block height
    pub height: Height,

    /// The transaction hash.
    pub txhash: String,

    /// Namespace for the Code
    pub codespace: String,

    /// Response code.
    pub code: u32,

    /// Result bytes, if any.
    pub data: String,

    /// The output of the application's logger (raw string). May be
    /// non-deterministic.
    pub raw_log: String,

    /// The output of the application's logger (typed). May be non-deterministic.
    pub logs: Vec<AbciMessageLog>,

    /// Additional information. May be non-deterministic.
    pub info: String,

    /// Amount of gas requested for transaction.
    pub gas_wanted: i64,

    /// Amount of gas consumed by transaction.
    pub gas_used: i64,

    /// The request transaction bytes.
    #[serde(skip)]
    // caused by prost_types/pbjson_types::Any conditional compilation, should be
    // removed once we align on tendermint::Any
    pub tx: Option<Any>,

    /// Time of the previous block. For heights > 1, it's the weighted median of
    /// the timestamps of the valid votes in the block.LastCommit. For height == 1,
    /// it's genesis time.
    pub timestamp: String,

    /// Events defines all the events emitted by processing a transaction. Note,
    /// these events include those emitted by processing all the messages and those
    /// emitted from the ante. Whereas Logs contains the events, with
    /// additional metadata, emitted only by processing the messages.
    pub events: Vec<Event>,
}

/// [`AuthInfo`] describes the fee and signer modes that are used to sign a transaction.
#[derive(Debug, Clone)]
pub struct AuthInfo {
    /// Defines the signing modes for the required signers.
    ///
    /// The number and order of elements must match the required signers from transaction
    /// [`TxBody`]’s messages. The first element is the primary signer and the one
    /// which pays the [`Fee`].
    pub signer_infos: Vec<SignerInfo>,
    /// [`Fee`] and gas limit for the transaction.
    ///
    /// The first signer is the primary signer and the one which pays the fee.
    /// The fee can be calculated based on the cost of evaluating the body and doing signature
    /// verification of the signers. This can be estimated via simulation.
    pub fee: Fee,
}

/// SignerInfo describes the public key and signing mode of a single top-level
/// signer.
#[derive(Debug, Clone, PartialEq)]
pub struct SignerInfo {
    /// public_key is the public key of the signer. It is optional for accounts
    /// that already exist in state. If unset, the verifier can use the required \
    /// signer address for this position and lookup the public key.
    pub public_key: Option<Any>,
    /// mode_info describes the signing mode of the signer and is a nested
    /// structure to support nested multisig pubkey's
    pub mode_info: ModeInfo,
    /// sequence is the sequence of the account, which describes the
    /// number of committed transactions signed by a given address. It is used to
    /// prevent replay attacks.
    pub sequence: u64,
}

/// ModeInfo describes the signing mode of a single or nested multisig signer.
#[derive(Debug, Clone, PartialEq)]
pub struct ModeInfo {
    /// sum is the oneof that specifies whether this represents a single or nested
    /// multisig signer
    pub sum: Sum,
}

/// sum is the oneof that specifies whether this represents a single or nested
/// multisig signer
#[derive(Debug, Clone, PartialEq)]
pub enum Sum {
    /// Single is the mode info for a single signer. It is structured as a message
    /// to allow for additional fields such as locale for SIGN_MODE_TEXTUAL in the
    /// future
    Single {
        /// mode is the signing mode of the single signer
        mode: i32,
    },
    /// Multi is the mode info for a multisig public key
    Multi {
        /// bitarray specifies which keys within the multisig are signing
        bitarray: BitVector,
        /// mode_infos is the corresponding modes of the signers of the multisig
        /// which could include nested multisig public keys
        mode_infos: Vec<ModeInfo>,
    },
}

/// Fee includes the amount of coins paid in fees and the maximum
/// gas to be used by the transaction. The ratio yields an effective "gasprice",
/// which must be above some miminum to be accepted into the mempool.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fee {
    /// amount is the amount of coins to be paid as a fee
    pub amount: Vec<Coin>,
    /// gas_limit is the maximum gas that can be used in transaction processing
    /// before an out of gas error occurs
    pub gas_limit: u64,
    /// if unset, the first signer is responsible for paying the fees. If set, the specified account must pay the fees.
    /// the payer must be a tx signer (and thus have signed this field in AuthInfo).
    /// setting this field does *not* change the ordering of required signers for the transaction.
    pub payer: String,
    /// if set, the fee payer (either the first signer or the value of the payer field) requests that a fee grant be used
    /// to pay fees instead of the fee payer's own balance. If an appropriate fee grant does not exist or the chain does
    /// not support fee grants, this will fail
    pub granter: String,
}

/// Coin defines a token with a denomination and an amount.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Coin {
    /// Coin denomination
    pub denom: String,
    /// Coin amount
    pub amount: u64,
}

impl Fee {
    pub fn new(utia_fee: u64, gas_limit: u64) -> Self {
        Fee {
            amount: vec![Coin {
                denom: BOND_DENOM.to_string(),
                amount: utia_fee,
            }],
            gas_limit,
            ..Default::default()
        }
    }
}

impl TryFrom<RawTxBody> for TxBody {
    type Error = Error;

    fn try_from(value: RawTxBody) -> Result<Self, Self::Error> {
        Ok(TxBody {
            messages: value.messages,
            memo: value.memo,
            timeout_height: value.timeout_height.try_into()?,
            extension_options: value.extension_options,
            non_critical_extension_options: value.non_critical_extension_options,
        })
    }
}

impl From<TxBody> for RawTxBody {
    fn from(value: TxBody) -> Self {
        RawTxBody {
            messages: value.messages,
            memo: value.memo,
            timeout_height: value.timeout_height.into(),
            extension_options: value.extension_options,
            non_critical_extension_options: value.non_critical_extension_options,
        }
    }
}

impl TryFrom<RawAuthInfo> for AuthInfo {
    type Error = Error;

    fn try_from(value: RawAuthInfo) -> Result<Self, Self::Error> {
        let signer_infos = value
            .signer_infos
            .into_iter()
            .map(TryFrom::try_from)
            .collect::<Result<_, Error>>()?;
        Ok(AuthInfo {
            signer_infos,
            fee: value.fee.ok_or(Error::MissingFee)?.try_into()?,
        })
    }
}

impl From<AuthInfo> for RawAuthInfo {
    fn from(value: AuthInfo) -> Self {
        let signer_infos = value.signer_infos.into_iter().map(Into::into).collect();
        #[allow(deprecated)] // tip is deprecated
        RawAuthInfo {
            signer_infos,
            fee: Some(value.fee.into()),
            tip: None,
        }
    }
}

impl TryFrom<RawTxResponse> for TxResponse {
    type Error = Error;

    fn try_from(response: RawTxResponse) -> Result<TxResponse, Self::Error> {
        Ok(TxResponse {
            height: response.height.try_into()?,
            txhash: response.txhash,
            codespace: response.codespace,
            code: response.code,
            data: response.data,
            raw_log: response.raw_log,
            logs: response.logs,
            info: response.info,
            gas_wanted: response.gas_wanted,
            gas_used: response.gas_used,
            tx: response.tx,
            timestamp: response.timestamp,
            events: response.events,
        })
    }
}

impl TryFrom<RawSignerInfo> for SignerInfo {
    type Error = Error;

    fn try_from(value: RawSignerInfo) -> Result<Self, Self::Error> {
        Ok(SignerInfo {
            public_key: value.public_key,
            mode_info: value.mode_info.ok_or(Error::MissingModeInfo)?.try_into()?,
            sequence: value.sequence,
        })
    }
}

impl From<SignerInfo> for RawSignerInfo {
    fn from(value: SignerInfo) -> Self {
        RawSignerInfo {
            public_key: value.public_key,
            mode_info: Some(value.mode_info.into()),
            sequence: value.sequence,
        }
    }
}

impl TryFrom<RawFee> for Fee {
    type Error = Error;

    fn try_from(value: RawFee) -> Result<Fee, Self::Error> {
        let amount = value
            .amount
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, Error>>()?;
        Ok(Fee {
            amount,
            gas_limit: value.gas_limit,
            payer: value.payer,
            granter: value.granter,
        })
    }
}

impl From<Fee> for RawFee {
    fn from(value: Fee) -> Self {
        let amount = value.amount.into_iter().map(Into::into).collect();
        RawFee {
            amount,
            gas_limit: value.gas_limit,
            payer: value.payer,
            granter: value.granter,
        }
    }
}

impl TryFrom<RawModeInfo> for ModeInfo {
    type Error = Error;

    fn try_from(value: RawModeInfo) -> Result<Self, Self::Error> {
        Ok(ModeInfo {
            sum: value.sum.ok_or(Error::MissingSum)?.try_into()?,
        })
    }
}

impl From<ModeInfo> for RawModeInfo {
    fn from(value: ModeInfo) -> Self {
        RawModeInfo {
            sum: Some(value.sum.into()),
        }
    }
}

impl TryFrom<RawSum> for Sum {
    type Error = Error;

    fn try_from(value: RawSum) -> Result<Self, Self::Error> {
        let sum = match value {
            RawSum::Single(Single { mode }) => Sum::Single { mode },
            RawSum::Multi(Multi {
                bitarray,
                mode_infos,
            }) => {
                let bitarray = bitarray.ok_or(Error::MissingBitarray)?.try_into()?;
                let mode_infos = mode_infos
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, Error>>()?;
                Sum::Multi {
                    bitarray,
                    mode_infos,
                }
            }
        };
        Ok(sum)
    }
}

impl From<Sum> for RawSum {
    fn from(value: Sum) -> Self {
        match value {
            Sum::Single { mode } => RawSum::Single(Single { mode }),
            Sum::Multi {
                bitarray,
                mode_infos,
            } => {
                let mode_infos = mode_infos.into_iter().map(Into::into).collect();
                RawSum::Multi(Multi {
                    bitarray: Some(bitarray.into()),
                    mode_infos,
                })
            }
        }
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

impl TryFrom<RawCoin> for Coin {
    type Error = Error;

    fn try_from(value: RawCoin) -> Result<Self, Self::Error> {
        Ok(Coin {
            denom: value.denom,
            amount: value
                .amount
                .parse()
                .map_err(|_| Error::InvalidCoinAmount(value.amount))?,
        })
    }
}

impl Protobuf<RawTxBody> for TxBody {}
impl Protobuf<RawAuthInfo> for AuthInfo {}
