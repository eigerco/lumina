//! Blocks within the chains of a Tendermint network

use celestia_proto::tendermint_celestia_mods::types::Block as RawBlock;
use serde::{Deserialize, Serialize};
use tendermint::block::{Commit, CommitSig, Header, Id};
use tendermint::signature::SIGNATURE_LENGTH;
use tendermint::{chain, evidence, vote, Vote};
use tendermint_proto::Protobuf;

use crate::consts::{genesis::MAX_CHAIN_ID_LEN, version};
use crate::hash::Hash;
use crate::{bail_validation, Error, Result, ValidateBasic, ValidationError};

mod data;

pub use data::Data;

pub(crate) const GENESIS_HEIGHT: u64 = 1;

/// The height of the block in Celestia network.
pub type Height = tendermint::block::Height;

/// Blocks consist of a header, transactions, votes (the commit), and a list of
/// evidence of malfeasance (i.e. signing conflicting votes).
///
/// This is a modified version of [`tendermint::block::Block`] which contains
/// [modifications](data-mod) that Celestia introduced.
///
/// [data-mod]: https://github.com/celestiaorg/celestia-core/blob/a1268f7ae3e688144a613c8a439dd31818aae07d/proto/tendermint/types/types.proto#L84-L104
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "RawBlock", into = "RawBlock")]
pub struct Block {
    /// Block header
    pub header: Header,

    /// Transaction data
    pub data: Data,

    /// Evidence of malfeasance
    pub evidence: evidence::List,

    /// Last commit, should be `None` for the initial block.
    pub last_commit: Option<Commit>,
}

impl Block {
    /// Builds a new [`Block`], based on the given [`Header`], [`Data`], evidence, and last commit.
    pub fn new(
        header: Header,
        data: Data,
        evidence: evidence::List,
        last_commit: Option<Commit>,
    ) -> Self {
        Block {
            header,
            data,
            evidence,
            last_commit,
        }
    }

    /// Get header
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Get data
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// Get evidence
    pub fn evidence(&self) -> &evidence::List {
        &self.evidence
    }

    /// Get last commit
    pub fn last_commit(&self) -> &Option<Commit> {
        &self.last_commit
    }
}

impl Protobuf<RawBlock> for Block {}

impl TryFrom<RawBlock> for Block {
    type Error = Error;

    fn try_from(value: RawBlock) -> Result<Self, Self::Error> {
        let header: Header = value
            .header
            .ok_or_else(tendermint::Error::missing_header)?
            .try_into()?;

        // If last_commit is the default Commit, it is considered nil by Go.
        let last_commit = value
            .last_commit
            .map(TryInto::try_into)
            .transpose()?
            .filter(|c| c != &Commit::default());

        Ok(Block::new(
            header,
            value
                .data
                .ok_or_else(tendermint::Error::missing_data)?
                .try_into()?,
            value
                .evidence
                .map(TryInto::try_into)
                .transpose()?
                .unwrap_or_default(),
            last_commit,
        ))
    }
}

impl From<Block> for RawBlock {
    fn from(value: Block) -> Self {
        RawBlock {
            header: Some(value.header.into()),
            data: Some(value.data.into()),
            evidence: Some(value.evidence.into()),
            last_commit: value.last_commit.map(Into::into),
        }
    }
}

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

impl ValidateBasic for Commit {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.height.value() >= GENESIS_HEIGHT {
            if is_zero(&self.block_id) {
                bail_validation!("block_id is zero")
            }

            if self.signatures.is_empty() {
                bail_validation!("no signatures in commit")
            }

            for commit_sig in &self.signatures {
                commit_sig.validate_basic()?;
            }
        }
        Ok(())
    }
}

impl ValidateBasic for CommitSig {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        match self {
            CommitSig::BlockIdFlagAbsent => (),
            CommitSig::BlockIdFlagCommit { signature, .. }
            | CommitSig::BlockIdFlagNil { signature, .. } => {
                if let Some(signature) = signature {
                    if signature.as_bytes().is_empty() {
                        bail_validation!("no signature in commit sig")
                    }
                    if signature.as_bytes().len() != SIGNATURE_LENGTH {
                        bail_validation!(
                            "signature ({:?}) length != required ({})",
                            signature.as_bytes(),
                            SIGNATURE_LENGTH
                        )
                    }
                } else {
                    bail_validation!("no signature in commit sig")
                }
            }
        }

        Ok(())
    }
}

/// An extension trait for the [`Commit`] to perform additional actions.
///
/// [`Commit`]: tendermint::block::Commit
pub trait CommitExt {
    /// Get the signed [`Vote`] from the [`Commit`] at the given index.
    ///
    /// [`Commit`]: tendermint::block::Commit
    /// [`Vote`]: tendermint::Vote
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>>;
}

impl CommitExt for Commit {
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>> {
        let sig =
            self.signatures
                .get(signature_idx)
                .cloned()
                .ok_or(Error::InvalidSignatureIndex(
                    signature_idx,
                    self.height.value(),
                ))?;

        let (validator_address, timestamp, signature) = match sig {
            CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp,
                signature,
            }
            | CommitSig::BlockIdFlagNil {
                validator_address,
                timestamp,
                signature,
            } => (validator_address, timestamp, signature),
            CommitSig::BlockIdFlagAbsent => return Err(Error::UnexpectedAbsentSignature),
        };

        let vote = Vote {
            vote_type: vote::Type::Precommit,
            height: self.height,
            round: self.round,
            block_id: Some(self.block_id),
            timestamp: Some(timestamp),
            validator_address,
            validator_index: signature_idx.try_into()?,
            signature,
            extension: Vec::new(),
            extension_signature: None,
        };

        Ok(vote.into_signable_vec(chain_id.clone()))
    }
}

fn is_zero(id: &Id) -> bool {
    matches!(id.hash, Hash::None)
        && matches!(id.part_set_header.hash, Hash::None)
        && id.part_set_header.total == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashExt;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_commit() -> Commit {
        serde_json::from_str(r#"{
          "height": "1",
          "round": 0,
          "block_id": {
            "hash": "17F7D5108753C39714DCA67E6A73CE855C6EA9B0071BBD4FFE5D2EF7F3973BFC",
            "parts": {
              "total": 1,
              "hash": "BEEBB79CDA7D0574B65864D3459FAC7F718B82496BD7FE8B6288BF0A98C8EA22"
            }
          },
          "signatures": [
            {
              "block_id_flag": 2,
              "validator_address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
              "timestamp": "2023-06-23T10:40:48.769228056Z",
              "signature": "HNn4c02eCt2+nGuBs55L8f3DAz9cgy9psLFuzhtg2XCWnlkt2V43TX2b54hQNi7C0fepBEteA3GC01aJM/JJCg=="
            }
          ]
        }"#).unwrap()
    }

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
    fn block_id_is_zero() {
        let mut block_id = sample_commit().block_id;
        assert!(!is_zero(&block_id));

        block_id.hash = Hash::None;
        assert!(!is_zero(&block_id));

        block_id.part_set_header.hash = Hash::None;
        assert!(!is_zero(&block_id));

        block_id.part_set_header.total = 0;
        assert!(is_zero(&block_id));
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

    #[test]
    fn commit_validate_basic() {
        sample_commit().validate_basic().unwrap();
    }

    #[test]
    fn commit_validate_invalid_block_id() {
        let mut commit = sample_commit();
        commit.block_id.hash = Hash::None;
        commit.block_id.part_set_header.hash = Hash::None;
        commit.block_id.part_set_header.total = 0;

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn commit_validate_no_signatures() {
        let mut commit = sample_commit();
        commit.signatures = vec![];

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn commit_validate_absent() {
        let mut commit = sample_commit();
        commit.signatures[0] = CommitSig::BlockIdFlagAbsent;

        commit.validate_basic().unwrap();
    }

    #[test]
    fn commit_validate_no_signature_in_sig() {
        let mut commit = sample_commit();
        let CommitSig::BlockIdFlagCommit {
            validator_address,
            timestamp,
            ..
        } = commit.signatures[0].clone()
        else {
            unreachable!()
        };
        commit.signatures[0] = CommitSig::BlockIdFlagCommit {
            signature: None,
            timestamp,
            validator_address,
        };

        commit.validate_basic().unwrap_err();
    }

    #[test]
    fn vote_sign_bytes() {
        let commit = sample_commit();

        let signable_bytes = commit
            .vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 0)
            .unwrap();

        assert_eq!(
            signable_bytes,
            vec![
                108u8, 8, 2, 17, 1, 0, 0, 0, 0, 0, 0, 0, 34, 72, 10, 32, 23, 247, 213, 16, 135, 83,
                195, 151, 20, 220, 166, 126, 106, 115, 206, 133, 92, 110, 169, 176, 7, 27, 189, 79,
                254, 93, 46, 247, 243, 151, 59, 252, 18, 36, 8, 1, 18, 32, 190, 235, 183, 156, 218,
                125, 5, 116, 182, 88, 100, 211, 69, 159, 172, 127, 113, 139, 130, 73, 107, 215,
                254, 139, 98, 136, 191, 10, 152, 200, 234, 34, 42, 12, 8, 176, 237, 213, 164, 6,
                16, 152, 250, 229, 238, 2, 50, 7, 112, 114, 105, 118, 97, 116, 101
            ]
        );
    }

    #[test]
    fn vote_sign_bytes_absent_signature() {
        let mut commit = sample_commit();
        commit.signatures[0] = CommitSig::BlockIdFlagAbsent;

        let res = commit.vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 0);

        assert!(matches!(res, Err(Error::UnexpectedAbsentSignature)));
    }

    #[test]
    fn vote_sign_bytes_non_existent_signature() {
        let commit = sample_commit();

        let res = commit.vote_sign_bytes(&"private".to_owned().try_into().unwrap(), 3);

        assert!(matches!(res, Err(Error::InvalidSignatureIndex(..))));
    }
}
