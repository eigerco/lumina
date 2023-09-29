use ed25519_consensus::SigningKey;
use tendermint::{
    block::{
        header::{Header, Version},
        parts, Commit, CommitSig,
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

#[derive(Debug, Clone)]
pub struct ExtendedHeaderGenerator {
    chain_id: chain::Id,
    key: SigningKey,
    current_header: Option<ExtendedHeader>,
}

/// [`ExtendedHeader`] generator for testing purposes.
///
/// **WARNING: ALL METHODS PANIC! DO NOT USE IT IN PRODUCTION!**
impl ExtendedHeaderGenerator {
    /// Creates new `ExtendedHeaderGenerator`.
    pub fn new() -> ExtendedHeaderGenerator {
        let chain_id: chain::Id = "private".try_into().unwrap();

        let key = SigningKey::new(rand::thread_rng());

        ExtendedHeaderGenerator {
            chain_id,
            key,
            current_header: None,
        }
    }

    /// Creates new `ExtendedHeaderGenerator` starting from specified height.
    ///
    /// ```ignore
    /// let mut gen = ExtendedHeaderGenerator::new_from_height(5);
    /// let header5 = gen.next();
    /// ```
    pub fn new_from_height(height: u64) -> ExtendedHeaderGenerator {
        let mut gen = ExtendedHeaderGenerator::new();
        gen.skip(height.saturating_sub(1));
        gen
    }

    /// Generates the next header.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ExtendedHeader {
        let header = match self.current_header {
            Some(ref header) => generate_next(header, &self.key),
            None => generate_genesis(&self.chain_id, &self.key),
        };

        self.current_header = Some(header.clone());
        header
    }

    /// Generate the next amount of headers.
    pub fn next_many(&mut self, amount: u64) -> Vec<ExtendedHeader> {
        let mut headers = Vec::with_capacity(amount as usize);

        for _ in 0..amount {
            headers.push(self.next());
        }

        headers
    }

    /// Generates the next header of the provided header.
    ///
    /// This can be used to create two headers of same height but different hash.
    ///
    /// ```ignore
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next();
    /// let header2 = gen.next();
    /// let another_header2 = gen.next_of(&header1);
    /// ```
    ///
    /// # Note
    ///
    /// This method does not change the state of `ExtendedHeaderGenerator`.
    pub fn next_of(&self, header: &ExtendedHeader) -> ExtendedHeader {
        generate_next(header, &self.key)
    }

    /// Generates the next amount of headers of the provided header.
    ///
    /// This can be used to create two chains of headers of same
    /// heights but different hashes.
    ///
    /// ```ignore
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next();
    /// let headers_2_to_12 = gen.next_many(10);
    /// let another_headers_2_to_12 = gen.next_many_of(&header1, 10);
    /// ```
    ///
    /// # Note
    ///
    /// This method does not change the state of `ExtendedHeaderGenerator`.
    pub fn next_many_of(&self, header: &ExtendedHeader, amount: u64) -> Vec<ExtendedHeader> {
        let mut headers = Vec::with_capacity(amount as usize);

        for _ in 0..amount {
            let current_header = headers.last().unwrap_or(header);
            let header = self.next_of(current_header);
            headers.push(header);
        }

        headers
    }

    /// Generates the another header of the same height but different hash.
    ///
    /// ```ignore
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next();
    /// let header2 = gen.next();
    /// let another_header2 = gen.another_of(&header2);
    /// ```
    ///
    /// # Note
    ///
    /// This method does not change the state of `ExtendedHeaderGenerator`.
    pub fn another_of(&self, header: &ExtendedHeader) -> ExtendedHeader {
        let mut header = header.to_owned();

        header.header.consensus_hash = Hash::Sha256(rand::random());
        header.commit.block_id.part_set_header =
            parts::Header::new(1, Hash::Sha256(rand::random())).expect("invalid PartSetHeader");

        hash_and_sign(&mut header, &self.key);

        header
    }

    /// Skips an amount of headers.
    pub fn skip(&mut self, amount: u64) {
        for _ in 0..amount {
            self.next();
        }
    }

    /// Create a "forked" generator for "forking" the chain.
    ///
    /// ```ignore
    /// let mut gen_chain1 = ExtendedHeaderGenerator::new();
    ///
    /// let header1 = gen_chain1.next();
    /// let header2 = gen_chain1.next();
    ///
    /// let mut gen_chain2 = gen_chain1.fork();
    ///
    /// let header3_chain1 = gen_chain1.next();
    /// let header3_chain2 = gen_chain2.next();
    /// ```
    ///
    /// # Note
    ///
    /// This is the same as clone, but the name describes the intention.
    pub fn fork(&self) -> ExtendedHeaderGenerator {
        self.clone()
    }
}

impl Default for ExtendedHeaderGenerator {
    fn default() -> Self {
        ExtendedHeaderGenerator::new()
    }
}

fn generate_genesis(chain_id: &chain::Id, signing_key: &SigningKey) -> ExtendedHeader {
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

    let mut header = ExtendedHeader {
        header: Header {
            version: Version {
                block: version::BLOCK_PROTOCOL,
                app: 1,
            },
            chain_id: chain_id.clone(),
            height: GENESIS_HEIGHT.try_into().unwrap(),
            time: Time::now(),
            last_block_id: None,
            last_commit_hash: Hash::default_sha256(),
            data_hash: Hash::None,
            validators_hash: Hash::None,
            next_validators_hash: Hash::None,
            consensus_hash: Hash::Sha256(rand::random()),
            app_hash: Hash::default_sha256()
                .as_bytes()
                .to_vec()
                .try_into()
                .unwrap(),
            last_results_hash: Hash::default_sha256(),
            evidence_hash: Hash::default_sha256(),
            proposer_address: validator_address,
        },
        commit: Commit {
            height: GENESIS_HEIGHT.try_into().unwrap(),
            round: 0_u16.into(),
            block_id: tendermint::block::Id {
                hash: Hash::None,
                part_set_header: parts::Header::new(1, Hash::Sha256(rand::random()))
                    .expect("invalid PartSetHeader"),
            },
            signatures: vec![CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp: Time::now(),
                signature: None,
            }],
        },
        validator_set,
        dah: DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
            column_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
        },
    };

    hash_and_sign(&mut header, signing_key);

    header
}

fn generate_next(current: &ExtendedHeader, signing_key: &SigningKey) -> ExtendedHeader {
    let validator_address = current.validator_set.validators()[0].address;

    let mut header = ExtendedHeader {
        header: Header {
            version: current.header.version,
            chain_id: current.header.chain_id.clone(),
            height: current.header.height.increment(),
            time: Time::now(),
            last_block_id: Some(current.commit.block_id),
            last_commit_hash: Hash::default_sha256(),
            data_hash: Hash::None,
            validators_hash: Hash::None,
            next_validators_hash: Hash::None,
            consensus_hash: Hash::Sha256(rand::random()),
            app_hash: Hash::default_sha256()
                .as_bytes()
                .to_vec()
                .try_into()
                .unwrap(),
            last_results_hash: Hash::default_sha256(),
            evidence_hash: Hash::default_sha256(),
            proposer_address: validator_address,
        },
        commit: Commit {
            height: current.header.height.increment(),
            round: 0_u16.into(),
            block_id: tendermint::block::Id {
                hash: Hash::None,
                part_set_header: parts::Header::new(1, Hash::Sha256(rand::random()))
                    .expect("invalid PartSetHeader"),
            },
            signatures: vec![CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp: Time::now(),
                signature: None,
            }],
        },
        validator_set: current.validator_set.clone(),
        dah: DataAvailabilityHeader {
            row_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
            column_roots: vec![NamespacedHash::empty_root(), NamespacedHash::empty_root()],
        },
    };

    hash_and_sign(&mut header, signing_key);

    current.verify(&header).expect("invalid header generated");

    header
}

fn hash_and_sign(header: &mut ExtendedHeader, signing_key: &SigningKey) {
    header.header.validators_hash = header.validator_set.hash();
    header.header.next_validators_hash = header.validator_set.hash();
    header.header.data_hash = header.dah.hash();
    header.commit.block_id.hash = header.header.hash();

    let vote_sign = header
        .commit
        .vote_sign_bytes(&header.header.chain_id, 0)
        .unwrap();
    let sig = signing_key.sign(&vote_sign).to_bytes();

    match header.commit.signatures[0] {
        CommitSig::BlockIdFlagAbsent => {}
        CommitSig::BlockIdFlagNil {
            ref mut signature, ..
        }
        | CommitSig::BlockIdFlagCommit {
            ref mut signature, ..
        } => {
            *signature = Some(Signature::new(sig).unwrap().unwrap());
        }
    }

    header.validate().expect("invalid header generated");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_blocks() {
        let mut gen = ExtendedHeaderGenerator::new();

        let genesis = gen.next();
        assert_eq!(genesis.height().value(), 1);

        let height2 = gen.next();
        assert_eq!(height2.height().value(), 2);

        let another_height2 = gen.next_of(&genesis);
        assert_eq!(another_height2.height().value(), 2);

        genesis.verify(&height2).unwrap();
        genesis.verify(&another_height2).unwrap();

        assert_ne!(height2.hash(), another_height2.hash());
    }

    #[test]
    fn generate_and_verify_range() {
        let mut gen = ExtendedHeaderGenerator::new();

        let genesis = gen.next();
        assert_eq!(genesis.height().value(), 1);

        let headers = gen.next_many(256);
        assert_eq!(headers.last().unwrap().height().value(), 257);

        genesis.verify_adjacent_range(&headers).unwrap();
        genesis.verify_range(&headers[10..]).unwrap();

        headers[0].verify_adjacent_range(&headers[1..]).unwrap();
        headers[0].verify_range(&headers[10..]).unwrap();

        headers[5].verify_adjacent_range(&headers[6..]).unwrap();
        headers[5].verify_range(&headers[10..]).unwrap();
    }

    #[test]
    fn generate_and_skip() {
        let mut gen = ExtendedHeaderGenerator::new();

        let genesis = gen.next();
        gen.skip(3);
        let header5 = gen.next();

        assert_eq!(genesis.height().value(), 1);
        assert_eq!(header5.height().value(), 5);
    }

    #[test]
    fn new_from_height() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();
        assert_eq!(header5.height().value(), 5);

        let mut gen = ExtendedHeaderGenerator::new_from_height(1);
        let header1 = gen.next();
        assert_eq!(header1.height().value(), 1);

        let mut gen = ExtendedHeaderGenerator::new_from_height(0);
        let header1 = gen.next();
        assert_eq!(header1.height().value(), 1);
    }

    #[test]
    fn generate_next_of() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let header6 = gen.next();
        let _header7 = gen.next();
        let another_header6 = gen.next_of(&header5);

        header5.verify(&header6).unwrap();
        header5.verify(&another_header6).unwrap();

        assert_eq!(header6.height().value(), 6);
        assert_eq!(another_header6.height().value(), 6);
        assert_ne!(header6.hash(), another_header6.hash());
    }

    #[test]
    fn generate_next_many_of() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let header6 = gen.next();
        let _header7 = gen.next();
        let another_header_6_to_10 = gen.next_many_of(&header5, 5);

        header5.verify(&header6).unwrap();
        header5
            .verify_adjacent_range(&another_header_6_to_10)
            .unwrap();

        assert_eq!(another_header_6_to_10.len(), 5);
        assert_eq!(header6.height().value(), 6);
        assert_eq!(another_header_6_to_10[0].height().value(), 6);
        assert_ne!(header6.hash(), another_header_6_to_10[0].hash());
    }

    #[test]
    fn gen_next_after_next_many_of() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let another_header_6_to_10 = gen.next_many_of(&header5, 5);
        // `next_of` and `next_many_of` does not change the state of the
        // generator, so `next` must return height 6 header.
        let header6 = gen.next();

        header5.verify(&header6).unwrap();
        header5
            .verify_adjacent_range(&another_header_6_to_10)
            .unwrap();

        assert_eq!(another_header_6_to_10.len(), 5);
        assert_eq!(header6.height().value(), 6);
        assert_eq!(another_header_6_to_10[0].height().value(), 6);
        assert_ne!(header6.hash(), another_header_6_to_10[0].hash());
    }

    #[test]
    fn generate_another_of() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let header6 = gen.next();

        let another_header6 = gen.another_of(&header6);

        header5.verify(&header6).unwrap();
        header5.verify(&another_header6).unwrap();
    }
}
