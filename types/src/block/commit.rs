//! Commitment types for blocks

use tendermint::block::{Commit, CommitSig, Id};
use tendermint::signature::SIGNATURE_LENGTH;
use tendermint::{chain, vote, Vote};

use crate::block::GENESIS_HEIGHT;
use crate::hash::Hash;
use crate::{bail_validation, Error, Result, ValidateBasic, ValidationError};

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

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use tendermint::block::{Commit, CommitSig};
    use wasm_bindgen::prelude::*;

    use crate::block::JsBlockId;
    use crate::signature::JsSignature;

    /// Commit contains the justification (ie. a set of signatures) that a block was
    /// committed by a set of validators.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Commit")]
    pub struct JsCommit {
        /// Block height
        pub height: u64,
        /// Round
        pub round: u32,
        /// Block ID
        pub block_id: JsBlockId,
        /// Signatures
        pub signatures: Vec<JsCommitSig>,
    }

    impl From<Commit> for JsCommit {
        fn from(value: Commit) -> Self {
            JsCommit {
                height: value.height.into(),
                round: value.round.into(),
                block_id: value.block_id.into(),
                signatures: value.signatures.into_iter().map(Into::into).collect(),
            }
        }
    }

    /// CommitSig represents a signature of a validator. It’s a part of the Commit and can
    /// be used to reconstruct the vote set given the validator set.
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "CommitSig")]
    pub struct JsCommitSig {
        /// vote type of a validator
        pub vote_type: JsCommitVoteType,
        /// vote, if received
        pub vote: Option<JsCommitVote>,
    }

    impl From<CommitSig> for JsCommitSig {
        fn from(value: CommitSig) -> Self {
            match value {
                CommitSig::BlockIdFlagAbsent => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagAbsent,
                    vote: None,
                },
                CommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagCommit,
                    vote: Some(JsCommitVote {
                        validator_address: validator_address.to_string(),
                        timestamp: timestamp.to_rfc3339(),
                        signature: signature.map(Into::into),
                    }),
                },
                CommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => JsCommitSig {
                    vote_type: JsCommitVoteType::BlockIdFlagNil,
                    vote: Some(JsCommitVote {
                        validator_address: validator_address.to_string(),
                        timestamp: timestamp.to_rfc3339(),
                        signature: signature.map(Into::into),
                    }),
                },
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    #[wasm_bindgen(js_name = "CommitVoteType")]
    #[allow(clippy::enum_variant_names)] // keep the tendermint names
    pub enum JsCommitVoteType {
        /// no vote was received from a validator.
        BlockIdFlagAbsent,
        /// voted for the Commit.BlockID.
        BlockIdFlagCommit,
        /// voted for nil
        BlockIdFlagNil,
    }

    /// Value of the validator vote
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "CommitVote")]
    pub struct JsCommitVote {
        /// Address of the voting validator
        pub validator_address: String,
        /// Timestamp
        pub timestamp: String,
        /// Signature
        pub signature: Option<JsSignature>,
    }
}

#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::block::{Commit as TendermintCommit, CommitSig as TendermintCommitSig};
    use uniffi::{Enum, Record};

    use crate::block::uniffi_types::BlockId;
    use crate::error::UniffiConversionError;
    use crate::signature::uniffi_types::Signature;
    use crate::state::UniffiAccountId;
    use crate::uniffi_types::Time;

    /// Commit contains the justification (ie. a set of signatures) that a block was
    /// committed by a set of validators.
    #[derive(Record)]
    pub struct Commit {
        /// Block height
        pub height: u64,
        /// Round
        pub round: u32,
        /// Block ID
        pub block_id: BlockId,
        /// Signatures
        pub signatures: Vec<CommitSig>,
    }

    impl TryFrom<TendermintCommit> for Commit {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintCommit) -> Result<Self, Self::Error> {
            Ok(Commit {
                height: value.height.value(),
                round: value.round.value(),
                block_id: value.block_id.into(),
                signatures: value
                    .signatures
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            })
        }
    }

    impl TryFrom<Commit> for TendermintCommit {
        type Error = UniffiConversionError;

        fn try_from(value: Commit) -> Result<Self, Self::Error> {
            Ok(TendermintCommit {
                height: value
                    .height
                    .try_into()
                    .map_err(|_| UniffiConversionError::HeaderHeightOutOfRange)?,
                round: value
                    .round
                    .try_into()
                    .map_err(|_| UniffiConversionError::InvalidRoundIndex)?,
                block_id: value.block_id.try_into()?,
                signatures: value
                    .signatures
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<Result<_, _>>()?,
            })
        }
    }

    uniffi::custom_type!(TendermintCommit, Commit, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.try_into().expect("valid tendermint timestamp")
    });

    /// CommitSig represents a signature of a validator. It’s a part of the Commit and can
    /// be used to reconstruct the vote set given the validator set.
    #[derive(Enum)]
    #[allow(clippy::enum_variant_names)] // keep the tendermint names
    pub enum CommitSig {
        /// no vote was received from a validator.
        BlockIdFlagAbsent,
        /// voted for the Commit.BlockID.
        BlockIdFlagCommit {
            /// Validator address
            validator_address: UniffiAccountId,
            /// Timestamp
            timestamp: Time,
            /// Signature of vote
            signature: Option<Signature>,
        },
        /// voted for nil
        BlockIdFlagNil {
            /// Validator address
            validator_address: UniffiAccountId,
            /// Timestamp
            timestamp: Time,
            /// Signature of vote
            signature: Option<Signature>,
        },
    }

    impl TryFrom<TendermintCommitSig> for CommitSig {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintCommitSig) -> Result<Self, Self::Error> {
            Ok(match value {
                TendermintCommitSig::BlockIdFlagAbsent => CommitSig::BlockIdFlagAbsent,
                TendermintCommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => CommitSig::BlockIdFlagCommit {
                    validator_address: validator_address.into(),
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(Into::into),
                },
                TendermintCommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => CommitSig::BlockIdFlagNil {
                    validator_address: validator_address.into(),
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(Into::into),
                },
            })
        }
    }

    impl TryFrom<CommitSig> for TendermintCommitSig {
        type Error = UniffiConversionError;

        fn try_from(value: CommitSig) -> Result<Self, Self::Error> {
            Ok(match value {
                CommitSig::BlockIdFlagAbsent => TendermintCommitSig::BlockIdFlagAbsent,
                CommitSig::BlockIdFlagCommit {
                    validator_address,
                    timestamp,
                    signature,
                } => TendermintCommitSig::BlockIdFlagCommit {
                    validator_address: validator_address.try_into()?,
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(TryInto::try_into).transpose()?,
                },
                CommitSig::BlockIdFlagNil {
                    validator_address,
                    timestamp,
                    signature,
                } => TendermintCommitSig::BlockIdFlagNil {
                    validator_address: validator_address.try_into()?,
                    timestamp: timestamp.try_into()?,
                    signature: signature.map(TryInto::try_into).transpose()?,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
