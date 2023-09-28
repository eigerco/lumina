use tendermint::{
    block::{
        header::{Header, Version},
        Commit, CommitSig,
    },
    chain,
    public_key::PublicKey,
    Signature, Time,
};

use crate::block::{CommitExt, GENESIS_HEIGHT};
use crate::consts::version;
use crate::hash::{Hash, HashExt};
use crate::nmt::{NamespacedHash, NamespacedHashExt};
use crate::{DataAvailabilityHeader, ExtendedHeader, ValidatorSet};

pub type SigningKey = ed25519_consensus::SigningKey;

pub fn generate_ed25519_key() -> SigningKey {
    SigningKey::new(&mut rand::thread_rng())
}

impl ExtendedHeader {
    /// Generates header of height 1 for testing purposes only.
    ///
    /// **WARNING: DO NOT USE IT FOR PRODUCTION**
    ///
    /// # Panics
    ///
    /// Panics on any error
    pub fn generate_genesis(chain_id: &str, signing_key: &SigningKey) -> ExtendedHeader {
        let chain_id: chain::Id = chain_id.try_into().expect("invalid chain ID");

        let pub_key_bytes = signing_key.verification_key().to_bytes();
        let pub_key = PublicKey::from_raw_ed25519(&pub_key_bytes).unwrap();

        let validator_address = tendermint::account::Id::new(rand::random());

        let validator_set = ValidatorSet::new(
            vec![tendermint::validator::Info {
                address: validator_address,
                pub_key,
                power: 5000_u32.into(),
                name: None,
                proposer_priority: 0_i64.into(),
            }],
            Some(tendermint::validator::Info {
                address: validator_address,
                pub_key,
                power: 5000_u32.into(),
                name: None,
                proposer_priority: 0_i64.into(),
            }),
        );

        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
            column_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
        };

        let header = Header {
            version: Version {
                block: version::BLOCK_PROTOCOL,
                app: 1,
            },
            chain_id: chain_id.clone(),
            height: GENESIS_HEIGHT.try_into().unwrap(),
            time: Time::now(),
            last_block_id: None,
            last_commit_hash: Hash::default_sha256(),
            data_hash: dah.hash(),
            validators_hash: validator_set.hash(),
            next_validators_hash: validator_set.hash(),
            consensus_hash: Hash::Sha256(rand::random()),
            app_hash: Hash::default_sha256()
                .as_bytes()
                .to_vec()
                .try_into()
                .unwrap(),
            last_results_hash: Hash::default_sha256(),
            evidence_hash: Hash::default_sha256(),
            proposer_address: validator_address,
        };

        let mut commit = Commit {
            height: GENESIS_HEIGHT.try_into().unwrap(),
            round: 0_u16.into(),
            block_id: tendermint::block::Id {
                hash: header.hash(),
                part_set_header: tendermint::block::parts::Header::new(
                    1,
                    Hash::Sha256(rand::random()),
                )
                .expect("invalid PartSetHeader"),
            },
            signatures: vec![CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp: Time::now(),
                signature: None,
            }],
        };

        let vote_sign = commit.vote_sign_bytes(&chain_id, 0).unwrap();
        let sig = signing_key.sign(&vote_sign).to_bytes();

        if let CommitSig::BlockIdFlagCommit {
            ref mut signature, ..
        } = commit.signatures[0]
        {
            *signature = Some(Signature::new(sig).unwrap().unwrap());
        }

        let header = ExtendedHeader {
            header,
            commit,
            validator_set,
            dah,
        };

        header.validate().expect("invalid genesis header generated");

        header
    }

    /// Generates the next header (`self.height + 1`) for testing purposes only.
    ///
    /// **WARNING: DO NOT USE IT FOR PRODUCTION**
    ///
    /// # Panics
    ///
    /// Panics on any error
    pub fn generate_next(&self, signing_key: &SigningKey) -> ExtendedHeader {
        let validator_address = self.validator_set.validators()[0].address;

        let dah = DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
            column_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
        };

        let header = Header {
            version: Version {
                block: version::BLOCK_PROTOCOL,
                app: 1,
            },
            chain_id: self.header.chain_id.clone(),
            height: self.header.height.increment(),
            time: Time::now(),
            last_block_id: Some(self.commit.block_id.clone()),
            last_commit_hash: Hash::default_sha256(),
            data_hash: dah.hash(),
            validators_hash: self.validator_set.hash(),
            next_validators_hash: self.validator_set.hash(),
            consensus_hash: Hash::Sha256(rand::random()),
            app_hash: Hash::default_sha256()
                .as_bytes()
                .to_vec()
                .try_into()
                .unwrap(),
            last_results_hash: Hash::default_sha256(),
            evidence_hash: Hash::default_sha256(),
            proposer_address: validator_address,
        };

        let mut commit = Commit {
            height: self.header.height.increment(),
            round: 0_u16.into(),
            block_id: tendermint::block::Id {
                hash: header.hash(),
                part_set_header: tendermint::block::parts::Header::new(
                    1,
                    Hash::Sha256(rand::random()),
                )
                .expect("invalid PartSetHeader"),
            },
            signatures: vec![CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp: Time::now(),
                signature: None,
            }],
        };

        let vote_sign = commit.vote_sign_bytes(&self.header.chain_id, 0).unwrap();
        let sig = signing_key.sign(&vote_sign).to_bytes();

        if let CommitSig::BlockIdFlagCommit {
            ref mut signature, ..
        } = commit.signatures[0]
        {
            *signature = Some(Signature::new(sig).unwrap().unwrap());
        }

        let header = ExtendedHeader {
            header,
            commit,
            validator_set: self.validator_set.clone(),
            dah,
        };

        header.validate().expect("invalid header generated");
        self.verify(&header).expect("invalid header generated");

        header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_blocks() {
        let key = generate_ed25519_key();
        let genesis = ExtendedHeader::generate_genesis("private", &key);

        let height2 = genesis.generate_next(&key);
        let another_height2 = genesis.generate_next(&key);

        genesis.verify(&height2).unwrap();
        genesis.verify(&another_height2).unwrap();

        assert_ne!(height2.hash(), another_height2.hash());
    }

    #[test]
    fn generate_and_verify_range() {
        let key = generate_ed25519_key();

        let mut headers = vec![ExtendedHeader::generate_genesis("private", &key)];

        for _ in 0..256 {
            let next = headers.last().unwrap().generate_next(&key);
            headers.push(next);
        }

        headers[0].verify_adjacent_range(&headers[1..]).unwrap();
        headers[0].verify_range(&headers[10..]).unwrap();

        headers[5].verify_adjacent_range(&headers[6..]).unwrap();
        headers[5].verify_range(&headers[10..]).unwrap();
    }
}
