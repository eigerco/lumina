use tendermint::block::parts::Header as PartSetHeader;
use tendermint::block::{Commit, CommitSig, Header, Id};
use tendermint::signature::SIGNATURE_LENGTH;
use tendermint::vote;
use tendermint::{chain, Hash, Vote};

use crate::consts::{genesis::MAX_CHAIN_ID_LEN, version};
use crate::{Error, Result, ValidateBasic, ValidationError, ValidationResult};

trait IsZero {
    fn is_zero(&self) -> bool;
}

impl IsZero for Id {
    fn is_zero(&self) -> bool {
        matches!(self.hash, Hash::None) && self.part_set_header.is_zero()
    }
}

impl IsZero for PartSetHeader {
    fn is_zero(&self) -> bool {
        matches!(self.hash, Hash::None) && self.total == 0
    }
}

impl ValidateBasic for Header {
    fn validate_basic(&self) -> ValidationResult<()> {
        if self.version.block != version::BLOCK_PROTOCOL {
            return Err(ValidationError::IncorrectBlockProtocol(self.version.block));
        }

        if self.chain_id.as_str().len() > MAX_CHAIN_ID_LEN {
            return Err(ValidationError::ChainIdTooLong(
                self.chain_id.as_str().into(),
            ));
        }

        if self.height.value() == 0 {
            return Err(ValidationError::ZeroHeight);
        }

        if self.last_block_id.is_none() && self.height.value() != 1 {
            return Err(ValidationError::MissingLastBlockId);
        }

        Ok(())
    }
}

impl ValidateBasic for Commit {
    fn validate_basic(&self) -> ValidationResult<()> {
        if self.height.value() > 0 {
            if self.block_id.is_zero() {
                return Err(ValidationError::CommitForNilBlock);
            }

            if self.signatures.is_empty() {
                return Err(ValidationError::NoSignaturesInCommit);
            }

            for commit_sig in &self.signatures {
                commit_sig.validate_basic()?;
            }
        }
        Ok(())
    }
}

impl ValidateBasic for CommitSig {
    fn validate_basic(&self) -> ValidationResult<()> {
        match self {
            CommitSig::BlockIdFlagAbsent => (),
            CommitSig::BlockIdFlagCommit { signature, .. }
            | CommitSig::BlockIdFlagNil { signature, .. } => {
                if let Some(signature) = signature {
                    if signature.as_bytes().is_empty() {
                        return Err(ValidationError::NoSignatureInCommitSig);
                    }
                    if signature.as_bytes().len() != SIGNATURE_LENGTH {
                        return Err(ValidationError::InvalidLengthSignature(
                            signature.clone().to_bytes(),
                        ));
                    }
                } else {
                    return Err(ValidationError::NoSignatureInCommitSig);
                }
            }
        }

        Ok(())
    }
}

pub trait CommitExt {
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>>;
}

impl CommitExt for Commit {
    fn vote_sign_bytes(&self, chain_id: &chain::Id, signature_idx: usize) -> Result<Vec<u8>> {
        let sig = self.signatures[signature_idx].clone();
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
        };
        Ok(vote.to_signable_vec(chain_id.clone())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commit() -> Commit {
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
        assert!(!block_id.is_zero());

        block_id.hash = Hash::None;
        assert!(!block_id.is_zero());

        block_id.part_set_header.hash = Hash::None;
        assert!(!block_id.is_zero());

        block_id.part_set_header.total = 0;
        assert!(block_id.is_zero());
    }

    #[test]
    fn header_validate_basic() {
        sample_header().validate_basic().unwrap();
    }

    #[test]
    fn header_validate_invalid_block_version() {
        let mut header = sample_header();
        header.version.block = 1;

        assert!(matches!(
            header.validate_basic(),
            Err(ValidationError::IncorrectBlockProtocol(..))
        ));
    }

    #[test]
    fn header_validate_zero_height() {
        let mut header = sample_header();
        header.height = 0u32.into();

        assert!(matches!(
            header.validate_basic(),
            Err(ValidationError::ZeroHeight)
        ));
    }

    #[test]
    fn header_validate_missing_last_block_id() {
        let mut header = sample_header();
        header.height = 2u32.into();

        assert!(matches!(
            header.validate_basic(),
            Err(ValidationError::MissingLastBlockId)
        ));
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

        assert!(matches!(
            commit.validate_basic(),
            Err(ValidationError::CommitForNilBlock)
        ));
    }

    #[test]
    fn commit_validate_no_signatures() {
        let mut commit = sample_commit();
        commit.signatures = vec![];

        assert!(matches!(
            commit.validate_basic(),
            Err(ValidationError::NoSignaturesInCommit)
        ));
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
        } = commit.signatures[0].clone() else {
            unreachable!()
        };
        commit.signatures[0] = CommitSig::BlockIdFlagCommit {
            signature: None,
            timestamp,
            validator_address,
        };

        assert!(matches!(
            commit.validate_basic(),
            Err(ValidationError::NoSignatureInCommitSig)
        ));
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
}
