#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use tendermint::evidence::{ConflictingBlock, Evidence};
    use wasm_bindgen::prelude::*;

    use crate::block::{JsSignedHeader, JsVote};
    use crate::validator_set::{JsValidatorInfo, JsValidatorSet};

    /// Duplicate vote evidence
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "DuplicateVoteEvidence")]
    pub struct JsDuplicateVoteEvidence {
        /// Vote A
        pub vote_a: JsVote,
        /// Vote B
        pub vote_b: JsVote,
        /// Total voting power
        pub total_voting_power: u64,
        /// Validator power
        pub validator_power: u64,
        /// Timestamp
        pub timestamp: String,
    }
    /// Conflicting block detected in light client attack
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "ConflictingBlock")]
    pub struct JsConflictingBlock {
        /// Signed header
        pub signed_header: JsSignedHeader,
        /// Validator set
        pub validator_set: JsValidatorSet,
    }

    impl From<ConflictingBlock> for JsConflictingBlock {
        fn from(value: ConflictingBlock) -> Self {
            JsConflictingBlock {
                signed_header: value.signed_header.into(),
                validator_set: value.validator_set.into(),
            }
        }
    }

    /// LightClient attack evidence
    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "LightClientAttackEvidence")]
    pub struct JsLightClientAttackEvidence {
        /// Conflicting block
        pub conflicting_block: JsConflictingBlock,
        /// Common height
        pub common_height: u64,
        /// Byzantine validators
        pub byzantine_validators: Vec<JsValidatorInfo>,
        /// Total voting power
        pub total_voting_power: u64,
        /// Timestamp
        pub timestamp: String,
    }

    #[wasm_bindgen(js_name = "AttackType")]
    pub enum JsAttackType {
        DuplicateVote,
        LightClient,
    }

    #[wasm_bindgen(js_name = Evidence)]
    pub struct JsEvidence(Evidence);

    #[wasm_bindgen]
    impl JsEvidence {
        pub fn attack_type(&self) -> JsAttackType {
            match &self.0 {
                Evidence::DuplicateVote(_) => JsAttackType::DuplicateVote,
                Evidence::LightClientAttack(_) => JsAttackType::LightClient,
            }
        }

        pub fn get_duplicate_vote_evidence(&self) -> Option<JsDuplicateVoteEvidence> {
            let Evidence::DuplicateVote(e) = &self.0 else {
                return None;
            };
            Some(JsDuplicateVoteEvidence {
                vote_a: e.vote_a.clone().into(),
                vote_b: e.vote_b.clone().into(),
                total_voting_power: e.total_voting_power.into(),
                validator_power: e.validator_power.into(),
                timestamp: e.timestamp.to_rfc3339(),
            })
        }

        pub fn get_light_client_attack_evidence(&self) -> Option<JsLightClientAttackEvidence> {
            let Evidence::LightClientAttack(e) = &self.0 else {
                return None;
            };
            Some(JsLightClientAttackEvidence {
                conflicting_block: e.conflicting_block.clone().into(),
                common_height: e.common_height.value(),
                byzantine_validators: e
                    .byzantine_validators
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                total_voting_power: e.total_voting_power.into(),
                timestamp: e.timestamp.to_rfc3339(),
            })
        }
    }

    impl From<Evidence> for JsEvidence {
        fn from(value: Evidence) -> Self {
            JsEvidence(value)
        }
    }
}

#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::evidence::{
        ConflictingBlock as TendermintConfliclingBlock, DuplicateVoteEvidence,
        Evidence as TendermintEvidence, LightClientAttackEvidence, List as TendermintEvidenceList,
    };
    use uniffi::{Enum, Record};

    use crate::block::uniffi_types::SignedHeader;
    use crate::error::UniffiConversionError;
    use crate::uniffi_types::{Time, Vote};
    use crate::validator_set::uniffi_types::{ValidatorInfo, ValidatorSet};

    /// Conflicting block detected in light client attack
    #[derive(Record)]
    pub struct ConflictingBlock {
        /// Signed header
        pub signed_header: SignedHeader,
        /// Validator set
        pub validator_set: ValidatorSet,
    }

    impl TryFrom<TendermintConfliclingBlock> for ConflictingBlock {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintConfliclingBlock) -> Result<Self, Self::Error> {
            Ok(ConflictingBlock {
                signed_header: value.signed_header.try_into()?,
                validator_set: value.validator_set.into(),
            })
        }
    }

    impl TryFrom<ConflictingBlock> for TendermintConfliclingBlock {
        type Error = UniffiConversionError;

        fn try_from(value: ConflictingBlock) -> Result<Self, Self::Error> {
            Ok(TendermintConfliclingBlock {
                signed_header: value.signed_header.try_into()?,
                validator_set: value.validator_set.try_into()?,
            })
        }
    }

    uniffi::custom_type!(TendermintEvidenceList, Vec<Evidence>, {
        remote,
        try_lift: |value| {
            let evidence : Vec<_> = value.into_iter().map(|e| e.try_into()).collect::<Result<_, _>>()?;
            Ok(TendermintEvidenceList::new(evidence))
        },
        lower: |value| value.into_vec().into_iter().map(|e| e.try_into()).collect::<Result<_,_>>().expect("valid timestamps")
    });

    /// Evidence of malfeasance by validators (i.e. signing conflicting votes or light client attack).
    #[allow(clippy::large_enum_variant)]
    #[derive(Enum)]
    pub enum Evidence {
        /// Duplicate vote evidence
        DuplicateVote {
            /// Vote A
            vote_a: Vote,
            /// Vote B
            vote_b: Vote,
            /// Total voting power
            total_voting_power: u64,
            /// Validator power
            validator_power: u64,
            /// Timestamp
            timestamp: Time,
        },
        /// LightClient attack evidence
        LightClientAttack {
            /// Conflicting block
            conflicting_block: ConflictingBlock,
            /// Common height
            common_height: u64,
            /// Byzantine validators
            byzantine_validators: Vec<ValidatorInfo>,
            /// Total voting power
            total_voting_power: u64,
            /// Timestamp
            timestamp: Time,
        },
    }

    impl TryFrom<TendermintEvidence> for Evidence {
        type Error = UniffiConversionError;

        fn try_from(value: TendermintEvidence) -> Result<Self, Self::Error> {
            Ok(match value {
                TendermintEvidence::DuplicateVote(e) => Evidence::DuplicateVote {
                    vote_a: e.vote_a.try_into()?,
                    vote_b: e.vote_b.try_into()?,
                    total_voting_power: e.total_voting_power.value(),
                    validator_power: e.validator_power.value(),
                    timestamp: e.timestamp.try_into()?,
                },
                TendermintEvidence::LightClientAttack(e) => Evidence::LightClientAttack {
                    conflicting_block: e.conflicting_block.try_into()?,
                    common_height: e.common_height.into(),
                    byzantine_validators: e
                        .byzantine_validators
                        .into_iter()
                        .map(|v| v.into())
                        .collect(),
                    total_voting_power: e.total_voting_power.value(),
                    timestamp: e.timestamp.try_into()?,
                },
            })
        }
    }

    impl TryFrom<Evidence> for TendermintEvidence {
        type Error = UniffiConversionError;
        fn try_from(value: Evidence) -> Result<Self, Self::Error> {
            match value {
                Evidence::DuplicateVote {
                    vote_a,
                    vote_b,
                    total_voting_power,
                    validator_power,
                    timestamp,
                } => {
                    let evidence = DuplicateVoteEvidence {
                        vote_a: vote_a.try_into()?,
                        vote_b: vote_b.try_into()?,
                        total_voting_power: total_voting_power
                            .try_into()
                            .map_err(|_| UniffiConversionError::InvalidVotingPower)?,
                        validator_power: validator_power
                            .try_into()
                            .map_err(|_| UniffiConversionError::InvalidVotingPower)?,
                        timestamp: timestamp.try_into()?,
                    };
                    Ok(TendermintEvidence::DuplicateVote(Box::new(evidence)))
                }
                Evidence::LightClientAttack {
                    conflicting_block,
                    common_height,
                    byzantine_validators,
                    total_voting_power,
                    timestamp,
                } => {
                    let evidence = LightClientAttackEvidence {
                        conflicting_block: conflicting_block.try_into()?,
                        common_height: common_height
                            .try_into()
                            .map_err(|_| UniffiConversionError::HeaderHeightOutOfRange)?,
                        byzantine_validators: byzantine_validators
                            .into_iter()
                            .map(|v| v.try_into())
                            .collect::<Result<_, _>>()?,
                        total_voting_power: total_voting_power
                            .try_into()
                            .map_err(|_| UniffiConversionError::InvalidVotingPower)?,
                        timestamp: timestamp.try_into()?,
                    };

                    Ok(TendermintEvidence::LightClientAttack(Box::new(evidence)))
                }
            }
        }
    }
}
