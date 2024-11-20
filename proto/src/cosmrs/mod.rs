use pbjson_types::Any;

use cosmos_sdk_proto::cosmos::base::v1beta1::Coin as CosmrsCoin;
use cosmos_sdk_proto::cosmos::crypto::multisig::v1beta1::CompactBitArray as CosmrsCompactBitArray;
use cosmos_sdk_proto::cosmos::tx::v1beta1::mode_info::Multi as CosmrsMulti;
use cosmos_sdk_proto::cosmos::tx::v1beta1::mode_info::Single as CosmrsSingle;
use cosmos_sdk_proto::cosmos::tx::v1beta1::mode_info::Sum as CosmrsModeInfoSum;
use cosmos_sdk_proto::cosmos::tx::v1beta1::AuthInfo as CosmrsAuthInfo;
use cosmos_sdk_proto::cosmos::tx::v1beta1::Fee as CosmrsFee;
use cosmos_sdk_proto::cosmos::tx::v1beta1::ModeInfo as CosmrsModeInfo;
use cosmos_sdk_proto::cosmos::tx::v1beta1::SignerInfo as CosmrsSignerInfo;
use cosmos_sdk_proto::cosmos::tx::v1beta1::Tip as CosmrsTip;
use cosmos_sdk_proto::cosmos::tx::v1beta1::TxBody as CosmrsTxBody;

use crate::cosmos::base::v1beta1::Coin;
use crate::cosmos::crypto::multisig::v1beta1::CompactBitArray;
use crate::cosmos::tx::v1beta1::mode_info::Multi;
use crate::cosmos::tx::v1beta1::mode_info::Single;
use crate::cosmos::tx::v1beta1::mode_info::Sum;
use crate::cosmos::tx::v1beta1::{AuthInfo, Fee, ModeInfo, SignerInfo, Tip, TxBody};

pub trait ProtobufAnyConvertable {
    type Target;

    fn convert(self) -> Self::Target;
}

impl ProtobufAnyConvertable for Any {
    type Target = cosmrs::Any;

    fn convert(self) -> Self::Target {
        cosmrs::Any {
            type_url: self.type_url,
            value: self.value.to_vec(),
        }
    }
}

impl From<TxBody> for CosmrsTxBody {
    fn from(value: TxBody) -> Self {
        let messages = value.messages.into_iter().map(|e| e.convert()).collect();
        let extension_options = value
            .extension_options
            .into_iter()
            .map(|e| e.convert())
            .collect();
        let non_critical_extension_options = value
            .non_critical_extension_options
            .into_iter()
            .map(|e| e.convert())
            .collect();
        CosmrsTxBody {
            messages,
            memo: value.memo,
            timeout_height: value.timeout_height,
            extension_options,
            non_critical_extension_options,
        }
    }
}

impl From<AuthInfo> for CosmrsAuthInfo {
    fn from(value: AuthInfo) -> Self {
        let signer_infos = value.signer_infos.into_iter().map(Into::into).collect();

        // tip field is deprecated, but we want to convert it anyway
        #[allow(deprecated)]
        CosmrsAuthInfo {
            signer_infos,
            fee: value.fee.map(Into::into),
            tip: value.tip.map(Into::into),
        }
    }
}

impl From<SignerInfo> for CosmrsSignerInfo {
    fn from(value: SignerInfo) -> Self {
        CosmrsSignerInfo {
            public_key: value.public_key.map(ProtobufAnyConvertable::convert),
            mode_info: value.mode_info.map(Into::into),
            sequence: value.sequence,
        }
    }
}

impl From<Fee> for CosmrsFee {
    fn from(value: Fee) -> Self {
        let amount = value.amount.into_iter().map(Into::into).collect();
        CosmrsFee {
            amount,
            gas_limit: value.gas_limit,
            payer: value.payer,
            granter: value.granter,
        }
    }
}

impl From<Tip> for CosmrsTip {
    fn from(value: Tip) -> Self {
        let amount = value.amount.into_iter().map(Into::into).collect();
        CosmrsTip {
            amount,
            tipper: value.tipper,
        }
    }
}

impl From<Coin> for CosmrsCoin {
    fn from(value: Coin) -> Self {
        CosmrsCoin {
            denom: value.denom,
            amount: value.amount,
        }
    }
}

impl From<ModeInfo> for CosmrsModeInfo {
    fn from(value: ModeInfo) -> Self {
        let sum = value.sum.map(|sum| match sum {
            Sum::Single(s) => CosmrsModeInfoSum::Single(s.into()),
            Sum::Multi(s) => CosmrsModeInfoSum::Multi(s.into()),
        });
        CosmrsModeInfo { sum }
    }
}

impl From<Single> for CosmrsSingle {
    fn from(value: Single) -> Self {
        CosmrsSingle { mode: value.mode }
    }
}

impl From<Multi> for CosmrsMulti {
    fn from(value: Multi) -> Self {
        let mode_infos = value.mode_infos.into_iter().map(Into::into).collect();
        CosmrsMulti {
            bitarray: value.bitarray.map(Into::into),
            mode_infos,
        }
    }
}

impl From<CompactBitArray> for CosmrsCompactBitArray {
    fn from(value: CompactBitArray) -> Self {
        CosmrsCompactBitArray {
            extra_bits_stored: value.extra_bits_stored,
            elems: value.elems,
        }
    }
}
