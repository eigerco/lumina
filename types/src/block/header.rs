use tendermint::block::Header;

use crate::block::GENESIS_HEIGHT;
use crate::consts::{genesis::MAX_CHAIN_ID_LEN, version};
use crate::{bail_validation, Result, ValidateBasic, ValidationError};

impl ValidateBasic for Header {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.version.block != version::BLOCK_PROTOCOL {
            bail_validation!(
                "version block ({}) != block protocol ({})",
                self.version.block,
                version::BLOCK_PROTOCOL,
            )
        }

        if self.chain_id.as_str().len() > MAX_CHAIN_ID_LEN {
            bail_validation!(
                "chain id ({}) len > maximum ({})",
                self.chain_id,
                MAX_CHAIN_ID_LEN
            )
        }

        if self.height.value() == 0 {
            bail_validation!("height == 0")
        }

        if self.height.value() == GENESIS_HEIGHT && self.last_block_id.is_some() {
            bail_validation!("last_block_id == Some() at height {GENESIS_HEIGHT}");
        }

        if self.height.value() != GENESIS_HEIGHT && self.last_block_id.is_none() {
            bail_validation!("last_block_id == None at height {}", self.height)
        }

        // NOTE: We do not validate `Hash` fields because they are type safe.
        // In Go implementation the validation passes if their length is 0 or 32.
        //
        // NOTE: We do not validate `app_hash` because if can be anything

        Ok(())
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use tendermint::block::header::Version;
    use tendermint::block::parts;
    use tendermint::block::signed_header::SignedHeader;
    use tendermint::block::Header;
    use wasm_bindgen::prelude::*;

    use crate::block::commit::JsCommit;
    use crate::block::JsBlockId;

    /// Version contains the protocol version for the blockchain and the application.
    #[derive(Clone, Copy, Debug)]
    #[wasm_bindgen(js_name = "ProtocolVersion")]
    pub struct JsHeaderVersion {
        /// blockchain version
        pub block: u64,
        /// app version
        pub app: u64,
    }

    impl From<Version> for JsHeaderVersion {
        fn from(value: Version) -> Self {
            JsHeaderVersion {
                block: value.block,
                app: value.app,
            }
        }
    }
    /// Block parts header
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "PartsHeader")]
    pub struct JsPartsHeader {
        /// Number of parts in this block
        pub total: u32,
        /// Hash of the parts set header
        pub hash: String,
    }

    impl From<parts::Header> for JsPartsHeader {
        fn from(value: parts::Header) -> Self {
            JsPartsHeader {
                total: value.total,
                hash: value.hash.to_string(),
            }
        }
    }

    /// Block Header values contain metadata about the block and about the consensus,
    /// as well as commitments to the data in the current block, the previous block,
    /// and the results returned by the application.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Header")]
    pub struct JsHeader {
        /// Header version
        pub version: JsHeaderVersion,
        /// Chain ID
        pub chain_id: String,
        /// Current block height
        pub height: u64,
        /// Current timestamp encoded as rfc3339
        pub time: String,
        /// Previous block info
        pub last_block_id: Option<JsBlockId>,
        /// Commit from validators from the last block
        pub last_commit_hash: Option<String>,
        /// Merkle root of transaction hashes
        pub data_hash: Option<String>,
        /// Validators for the current block
        pub validators_hash: String,
        /// Validators for the next block
        pub next_validators_hash: String,
        /// Consensus params for the current block
        pub consensus_hash: String,
        /// State after txs from the previous block
        pub app_hash: String,
        /// Root hash of all results from the txs from the previous block
        pub last_results_hash: Option<String>,
        /// Hash of evidence included in the block
        pub evidence_hash: Option<String>,
        /// Original proposer of the block
        pub proposer_address: String,
    }

    impl From<Header> for JsHeader {
        fn from(value: Header) -> Self {
            JsHeader {
                version: value.version.into(),
                chain_id: value.chain_id.to_string(),
                height: value.height.value(),
                time: value.time.to_rfc3339(),
                last_block_id: value.last_block_id.map(Into::into),
                last_commit_hash: value.last_commit_hash.map(|h| h.to_string()),
                data_hash: value.data_hash.map(|h| h.to_string()),
                validators_hash: value.validators_hash.to_string(),
                next_validators_hash: value.next_validators_hash.to_string(),
                consensus_hash: value.consensus_hash.to_string(),
                app_hash: value.app_hash.to_string(),
                last_results_hash: value.last_results_hash.map(|h| h.to_string()),
                evidence_hash: value.evidence_hash.map(|h| h.to_string()),
                proposer_address: value.proposer_address.to_string(),
            }
        }
    }

    /// Signed block headers
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "SignedHeader")]
    pub struct JsSignedHeader {
        /// Signed block headers
        pub header: JsHeader,
        /// Commit containing signatures for the header
        pub commit: JsCommit,
    }

    impl From<SignedHeader> for JsSignedHeader {
        fn from(value: SignedHeader) -> Self {
            JsSignedHeader {
                header: value.header.into(),
                commit: value.commit.into(),
            }
        }
    }
}

#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::block::parts::Header as TendermintPartsHeader;
    use tendermint::block::signed_header::SignedHeader as TendermintSignedHeader;
    use tendermint::block::{Header as TendermintHeader, Height};
    use uniffi::Record;

    use crate::block::commit::uniffi_types::Commit;
    use crate::block::uniffi_types::BlockId;
    use crate::error::UniffiConversionError;
    use crate::hash::uniffi_types::AppHash;
    use crate::hash::Hash;
    use crate::state::UniffiAccountId;
    use crate::uniffi_types::{ChainId, Time};

    /// Version contains the protocol version for the blockchain and the application.
    pub type HeaderVersion = tendermint::block::header::Version;

    /// Version contains the protocol version for the blockchain and the application.
    #[uniffi::remote(Record)]
    pub struct HeaderVersion {
        /// blockchain version
        pub block: u64,
        /// app version
        pub app: u64,
    }

    /// Signed block headers
    #[derive(Record)]
    pub struct SignedHeader {
        /// Signed block headers
        pub header: Header,
        /// Commit containing signatures for the header
        pub commit: Commit,
    }

    impl TryFrom<TendermintSignedHeader> for SignedHeader {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintSignedHeader) -> Result<Self, Self::Error> {
            Ok(SignedHeader {
                header: value.header.into(),
                commit: value.commit.try_into()?,
            })
        }
    }

    impl TryFrom<SignedHeader> for TendermintSignedHeader {
        type Error = UniffiConversionError;

        fn try_from(value: SignedHeader) -> Result<Self, Self::Error> {
            TendermintSignedHeader::new(value.header.try_into()?, value.commit.try_into()?)
                .map_err(|_| UniffiConversionError::InvalidSignedHeader)
        }
    }

    /// Block Header values contain metadata about the block and about the consensus,
    /// as well as commitments to the data in the current block, the previous block,
    /// and the results returned by the application.
    #[derive(Record)]
    pub struct Header {
        /// Header version
        pub version: HeaderVersion,
        /// Chain ID
        pub chain_id: ChainId,
        /// Current block height
        pub height: Height,
        /// Current timestamp
        pub time: Time,
        /// Previous block info
        pub last_block_id: Option<BlockId>,
        /// Commit from validators from the last block
        pub last_commit_hash: Option<Hash>,
        /// Merkle root of transaction hashes
        pub data_hash: Option<Hash>,
        /// Validators for the current block
        pub validators_hash: Hash,
        /// Validators for the next block
        pub next_validators_hash: Hash,
        /// Consensus params for the current block
        pub consensus_hash: Hash,
        /// State after txs from the previous block
        pub app_hash: AppHash,
        /// Root hash of all results from the txs from the previous block
        pub last_results_hash: Option<Hash>,
        /// Hash of evidence included in the block
        pub evidence_hash: Option<Hash>,
        /// Original proposer of the block
        pub proposer_address: UniffiAccountId,
    }

    impl TryFrom<Header> for TendermintHeader {
        type Error = UniffiConversionError;

        fn try_from(value: Header) -> std::result::Result<Self, Self::Error> {
            Ok(TendermintHeader {
                version: value.version,
                chain_id: value.chain_id.try_into()?,
                height: value.height,
                time: value.time.try_into()?,
                last_block_id: value.last_block_id.map(TryInto::try_into).transpose()?,
                last_commit_hash: value.last_commit_hash,
                data_hash: value.data_hash,
                validators_hash: value.validators_hash,
                next_validators_hash: value.next_validators_hash,
                consensus_hash: value.consensus_hash,
                app_hash: value.app_hash.try_into()?,
                last_results_hash: value.last_results_hash,
                evidence_hash: value.evidence_hash,
                proposer_address: value.proposer_address.try_into()?,
            })
        }
    }

    impl From<TendermintHeader> for Header {
        fn from(value: TendermintHeader) -> Self {
            Header {
                version: value.version,
                chain_id: value.chain_id.into(),
                height: value.height,
                time: value.time.try_into().expect("valid time in tendermint"),
                last_block_id: value.last_block_id.map(Into::into),
                last_commit_hash: value.last_commit_hash,
                data_hash: value.data_hash,
                validators_hash: value.validators_hash,
                next_validators_hash: value.next_validators_hash,
                consensus_hash: value.consensus_hash,
                app_hash: value.app_hash.into(),
                last_results_hash: value.last_results_hash,
                evidence_hash: value.evidence_hash,
                proposer_address: value.proposer_address.into(),
            }
        }
    }

    uniffi::custom_type!(TendermintHeader, Header, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });

    /// Block parts header
    #[derive(Record)]
    pub struct PartsHeader {
        /// Number of parts in this block
        pub total: u32,
        /// Hash of the parts set header
        pub hash: Hash,
    }

    impl TryFrom<PartsHeader> for TendermintPartsHeader {
        type Error = UniffiConversionError;

        fn try_from(value: PartsHeader) -> Result<Self, Self::Error> {
            TendermintPartsHeader::new(value.total, value.hash)
                .map_err(|e| UniffiConversionError::InvalidPartsHeader { msg: e.to_string() })
        }
    }

    impl From<TendermintPartsHeader> for PartsHeader {
        fn from(value: TendermintPartsHeader) -> Self {
            PartsHeader {
                total: value.total,
                hash: value.hash,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{Hash, HashExt};
    use tendermint::block::Id;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_header() -> Header {
        serde_json::from_str(r#"{
          "version": {
            "block": "11",
            "app": "1"
          },
          "chain_id": "private",
          "height": "1",
          "time": "2023-06-23T10:40:48.410305119Z",
          "last_block_id": {
            "hash": "",
            "parts": {
              "total": 0,
              "hash": ""
            }
          },
          "last_commit_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "data_hash": "3D96B7D238E7E0456F6AF8E7CDF0A67BD6CF9C2089ECB559C659DCAA1F880353",
          "validators_hash": "64AEB6CA415A37540650FC04471974CE4FE88884CDD3300DF7BB27C1786871E9",
          "next_validators_hash": "64AEB6CA415A37540650FC04471974CE4FE88884CDD3300DF7BB27C1786871E9",
          "consensus_hash": "C0B6A634B72AE9687EA53B6D277A73ABA1386BA3CFC6D0F26963602F7F6FFCD6",
          "app_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "last_results_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "evidence_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
          "proposer_address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53"
        }"#).unwrap()
    }

    #[test]
    fn header_validate_basic() {
        sample_header().validate_basic().unwrap();
    }

    #[test]
    fn header_validate_invalid_block_version() {
        let mut header = sample_header();
        header.version.block = 1;

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_zero_height() {
        let mut header = sample_header();
        header.height = 0u32.into();

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_missing_last_block_id() {
        let mut header = sample_header();
        header.height = 2u32.into();

        header.validate_basic().unwrap_err();
    }

    #[test]
    fn header_validate_genesis_with_last_block_id() {
        let mut header = sample_header();

        header.last_block_id = Some(Id {
            hash: Hash::default_sha256(),
            ..Id::default()
        });

        header.validate_basic().unwrap_err();
    }
}
