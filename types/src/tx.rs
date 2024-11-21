//! Types associated with submitting and querying transaction

#[cfg(feature = "tonic")]
use pbjson_types::Any;
#[cfg(not(feature = "tonic"))]
use prost_types::Any;

use celestia_tendermint_proto::Protobuf;
use celestia_proto::cosmos::tx::v1beta1::{
    AuthInfo as RawAuthInfo, Fee, SignerInfo, TxBody as RawTxBody,
};
use celestia_tendermint::block::Height;

use crate::Error;

type Signature = Vec<u8>;

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
        Ok(AuthInfo {
            signer_infos: value.signer_infos,
            fee: value.fee.ok_or(Error::MissingFee)?,
        })
    }
}

impl From<AuthInfo> for RawAuthInfo {
    fn from(value: AuthInfo) -> Self {
        #[allow(deprecated)] // tip is deprecated
        RawAuthInfo {
            signer_infos: value.signer_infos,
            fee: Some(value.fee),
            tip: None,
        }
    }
}

impl Protobuf<RawTxBody> for TxBody {}
