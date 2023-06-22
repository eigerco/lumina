use tendermint::block::CommitSig;
use tendermint::crypto::default::signature::Verifier;
use tendermint::validator::Set;
use tendermint::{block, chain};

use crate::{CommitExt, Error, Result, ValidateBasic, ValidationError, ValidationResult};

impl ValidateBasic for Set {
    fn validate_basic(&self) -> ValidationResult<()> {
        if self.validators().is_empty() {
            return Err(ValidationError::ValidatorsSetEmpty);
        }

        if self.proposer().is_none() {
            return Err(ValidationError::ValidatorsSetProposerMissing);
        }

        Ok(())
    }
}

pub trait ValidatorSetExt {
    fn verify_commit_light(
        &self,
        chain_id: &chain::Id,
        height: &block::Height,
        commit: &block::Commit,
    ) -> Result<()>;
}

impl ValidatorSetExt for Set {
    fn verify_commit_light(
        &self,
        chain_id: &chain::Id,
        height: &block::Height,
        commit: &block::Commit,
    ) -> Result<()> {
        if self.validators().len() != commit.signatures.len() {
            Err(ValidationError::ValidatorsAndSignaturesMismatch(
                self.validators().len(),
                commit.signatures.len(),
            ))?;
        }

        if height != &commit.height {
            Err(ValidationError::HeaderAndCommitHeightMismatch(
                *height,
                commit.height,
            ))?;
        }

        let mut tallied_voting_power = 0;
        let voting_power_needed = self.total_voting_power().value() * 2 / 3;

        for (n, (validator, commit_sig)) in self
            .validators()
            .iter()
            .zip(commit.signatures.iter())
            .enumerate()
        {
            let signature = match commit_sig {
                CommitSig::BlockIdFlagCommit { signature, .. } if signature.is_some() => {
                    signature.as_ref().unwrap()
                }
                CommitSig::BlockIdFlagCommit { .. } => {
                    Err(ValidationError::NoSignatureInCommitSig)?
                }
                // not commiting for the block
                _ => continue,
            };
            let vote_sign = commit.vote_sign_bytes(chain_id, n)?;
            validator.verify_signature::<Verifier>(&vote_sign, signature)?;

            tallied_voting_power += validator.power();
            if tallied_voting_power > voting_power_needed {
                return Ok(());
            }
        }

        Err(Error::NotEnoughVotingPower(
            tallied_voting_power,
            voting_power_needed,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tendermint_proto::v0_34::types::ValidatorSet as RawValidatorSet;

    fn sample_commit() -> block::Commit {
        serde_json::from_str(r#"{
          "height": 1,
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

    fn sample_validator_set() -> Set {
        serde_json::from_str::<RawValidatorSet>(
            r#"{
              "validators": [
                {
                  "address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
                  "pub_key": {
                    "type": "tendermint/PubKeyEd25519",
                    "value": "yvrJ+hVxB/nh6sKTG+rrrpzyJgr4bxZ5KXM6VEw3t8w="
                  },
                  "voting_power": "5000",
                  "proposer_priority": "0"
                }
              ],
              "proposer": {
                "address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
                "pub_key": {
                  "type": "tendermint/PubKeyEd25519",
                  "value": "yvrJ+hVxB/nh6sKTG+rrrpzyJgr4bxZ5KXM6VEw3t8w="
                },
                "voting_power": "5000",
                "proposer_priority": "0"
              }
            }"#,
        )
        .unwrap()
        .try_into()
        .unwrap()
    }

    fn sample_validator_set_no_validators() -> Set {
        serde_json::from_str::<RawValidatorSet>(
            r#"{
              "validators": [],
              "proposer": {
                "address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
                "pub_key": {
                  "type": "tendermint/PubKeyEd25519",
                  "value": "yvrJ+hVxB/nh6sKTG+rrrpzyJgr4bxZ5KXM6VEw3t8w="
                },
                "voting_power": "5000",
                "proposer_priority": "0"
              }
            }"#,
        )
        .unwrap()
        .try_into()
        .unwrap()
    }

    fn sample_validator_set_no_proposer() -> Set {
        serde_json::from_str::<RawValidatorSet>(
            r#"{
              "validators": [
                {
                  "address": "F1F83230835AA69A1AD6EA68C6D894A4106B8E53",
                  "pub_key": {
                    "type": "tendermint/PubKeyEd25519",
                    "value": "yvrJ+hVxB/nh6sKTG+rrrpzyJgr4bxZ5KXM6VEw3t8w="
                  },
                  "voting_power": "5000",
                  "proposer_priority": "0"
                }
              ]
            }"#,
        )
        .unwrap()
        .try_into()
        .unwrap()
    }

    #[test]
    fn validate_correct() {
        sample_validator_set().validate_basic().unwrap();
    }

    #[test]
    fn validate_validators_missing() {
        assert!(matches!(
            sample_validator_set_no_validators().validate_basic(),
            Err(ValidationError::ValidatorsSetEmpty)
        ));
    }

    #[test]
    fn validate_proposer_missing() {
        assert!(matches!(
            sample_validator_set_no_proposer().validate_basic(),
            Err(ValidationError::ValidatorsSetProposerMissing)
        ));
    }

    #[test]
    fn verify_commit_light_success() {
        let commit = sample_commit();
        let val_set = sample_validator_set();

        val_set
            .verify_commit_light(
                &"private".to_string().try_into().unwrap(),
                &1u32.into(),
                &commit,
            )
            .unwrap();
    }

    #[test]
    fn verify_commit_light_validators_and_signatures_mismatch() {
        let mut commit = sample_commit();
        let val_set = sample_validator_set();
        commit.signatures.push(commit.signatures[0].clone());

        let result = val_set.verify_commit_light(
            &"private".to_string().try_into().unwrap(),
            &1u32.into(),
            &commit,
        );

        assert!(matches!(
            result,
            Err(Error::Validation(
                ValidationError::ValidatorsAndSignaturesMismatch(..)
            ))
        ));
    }

    #[test]
    fn verify_commit_light_commit_height_mismatch() {
        let mut commit = sample_commit();
        let val_set = sample_validator_set();
        commit.height = 2u32.into();

        let result = val_set.verify_commit_light(
            &"private".to_string().try_into().unwrap(),
            &1u32.into(),
            &commit,
        );

        assert!(matches!(
            result,
            Err(Error::Validation(
                ValidationError::HeaderAndCommitHeightMismatch(..)
            ))
        ));
    }
}
