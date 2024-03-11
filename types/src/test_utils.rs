//! Utilities for writing tests.

use celestia_tendermint::block::header::{Header, Version};
use celestia_tendermint::block::{parts, Commit, CommitSig};
use celestia_tendermint::public_key::PublicKey;
use celestia_tendermint::{chain, Signature, Time};
use ed25519_consensus::SigningKey;
use rand::RngCore;

use crate::block::{CommitExt, GENESIS_HEIGHT};
pub use crate::byzantine::test_utils::corrupt_eds;
use crate::consts::appconsts::{SHARE_INFO_BYTES, SHARE_SIZE};
use crate::consts::version;
use crate::hash::{Hash, HashExt};
use crate::nmt::{Namespace, NS_SIZE};
use crate::{DataAvailabilityHeader, ExtendedDataSquare, ExtendedHeader, ValidatorSet};

/// [`ExtendedHeader`] generator for testing purposes.
///
/// **WARNING: ALL METHODS PANIC! DO NOT USE IT IN PRODUCTION!**
#[derive(Debug, Clone)]
pub struct ExtendedHeaderGenerator {
    chain_id: chain::Id,
    key: SigningKey,
    current_header: Option<ExtendedHeader>,
}

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
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
    /// let mut gen = ExtendedHeaderGenerator::new_from_height(5);
    /// let header5 = gen.next();
    /// ```
    pub fn new_from_height(height: u64) -> ExtendedHeaderGenerator {
        let prev_height = height.saturating_sub(1);
        let mut gen = ExtendedHeaderGenerator::new();

        gen.current_header = if prev_height == 0 {
            None
        } else {
            Some(generate_new(prev_height, &gen.chain_id, &gen.key, None))
        };

        gen
    }

    /// Generates the next header.
    ///
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next();
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ExtendedHeader {
        let header = match self.current_header {
            Some(ref header) => generate_next(1, header, &self.key, None),
            None => generate_new(GENESIS_HEIGHT, &self.chain_id, &self.key, None),
        };

        self.current_header = Some(header.clone());
        header
    }

    /// Generates the next header with the given [`DataAvailabilityHeader`]
    ///
    /// ```no_run
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    /// # fn generate_dah() -> celestia_types::DataAvailabilityHeader {
    /// #    unimplemented!();
    /// # }
    ///
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next_with_dah(generate_dah());
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn next_with_dah(&mut self, dah: DataAvailabilityHeader) -> ExtendedHeader {
        let header = match self.current_header {
            Some(ref header) => generate_next(1, header, &self.key, Some(dah)),
            None => generate_new(GENESIS_HEIGHT, &self.chain_id, &self.key, Some(dah)),
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
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
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
        generate_next(1, header, &self.key, None)
    }

    /// Generates the next header of the provided header with the given [`DataAvailabilityHeader`].
    ///
    /// This can be used to create two headers of same height but different hash.
    ///
    /// ```no_run
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    /// # fn generate_dah() -> celestia_types::DataAvailabilityHeader {
    /// #    unimplemented!();
    /// # }
    ///
    /// let mut gen = ExtendedHeaderGenerator::new();
    /// let header1 = gen.next();
    /// let header2 = gen.next();
    /// let another_header2 = gen.next_of_with_dah(&header1, generate_dah());
    /// ```
    ///
    /// # Note
    ///
    /// This method does not change the state of `ExtendedHeaderGenerator`.
    pub fn next_of_with_dah(
        &self,
        header: &ExtendedHeader,
        dah: DataAvailabilityHeader,
    ) -> ExtendedHeader {
        generate_next(1, header, &self.key, Some(dah))
    }

    /// Generates the next amount of headers of the provided header.
    ///
    /// This can be used to create two chains of headers of same
    /// heights but different hashes.
    ///
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
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
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
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
        header.validate().expect("invalid header generated");

        header
    }

    /// Skips an amount of headers.
    pub fn skip(&mut self, amount: u64) {
        if amount == 0 {
            return;
        }

        let header = match self.current_header {
            Some(ref header) => generate_next(amount, header, &self.key, None),
            None => generate_new(amount, &self.chain_id, &self.key, None),
        };

        self.current_header = Some(header.clone());
    }

    /// Create a "forked" generator for "forking" the chain.
    ///
    /// ```
    /// use celestia_types::test_utils::ExtendedHeaderGenerator;
    ///
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

/// Invalidate the provided header.
///
/// This can be combined with [`unverify`]
pub fn invalidate(header: &mut ExtendedHeader) {
    // One way of invalidate `ExtendedHeader` but still passes the
    // verification check, is to clear `dah` but keep `data_hash` unchanged.
    header.dah = DataAvailabilityHeader::new_unchecked(Vec::new(), Vec::new());

    header.validate().unwrap_err();
}

/// Unverify the provided header.
///
/// This can be combined with [`invalidate`].
pub fn unverify(header: &mut ExtendedHeader) {
    let was_invalidated = header.validate().is_err();

    // One way to unverify `ExtendedHeader` but still passes the
    // validation check, is to sign it with a new key.
    let key = SigningKey::new(rand::thread_rng());
    let pub_key_bytes = key.verification_key().to_bytes();
    let pub_key = PublicKey::from_raw_ed25519(&pub_key_bytes).unwrap();
    let validator_address = celestia_tendermint::account::Id::new(rand::random());

    header.header.proposer_address = validator_address;

    header.validator_set = ValidatorSet::new(
        vec![celestia_tendermint::validator::Info {
            address: validator_address,
            pub_key,
            power: 5000_u32.into(),
            name: None,
            proposer_priority: 0_i64.into(),
        }],
        Some(celestia_tendermint::validator::Info {
            address: validator_address,
            pub_key,
            power: 5000_u32.into(),
            name: None,
            proposer_priority: 0_i64.into(),
        }),
    );

    hash_and_sign(header, &key);

    if was_invalidated {
        invalidate(header);
    } else {
        header.validate().expect("invalid header generated");
    }
}

/// Generate a properly encoded [`ExtendedDataSquare`] with random data.
pub fn generate_eds(square_width: usize) -> ExtendedDataSquare {
    let ns = Namespace::const_v0(rand::random());
    let ods_width = square_width / 2;

    let shares: Vec<_> = (0..ods_width * ods_width)
        .map(|_| {
            [
                ns.as_bytes(),
                &[0; SHARE_INFO_BYTES][..],
                &random_bytes(SHARE_SIZE - NS_SIZE - SHARE_INFO_BYTES)[..],
            ]
            .concat()
        })
        .collect();

    ExtendedDataSquare::from_ods(shares).unwrap()
}

pub(crate) fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

fn generate_new(
    height: u64,
    chain_id: &chain::Id,
    signing_key: &SigningKey,
    dah: Option<DataAvailabilityHeader>,
) -> ExtendedHeader {
    assert!(height >= GENESIS_HEIGHT);

    let pub_key_bytes = signing_key.verification_key().to_bytes();
    let pub_key = PublicKey::from_raw_ed25519(&pub_key_bytes).unwrap();
    let validator_address = celestia_tendermint::account::Id::new(rand::random());

    let last_block_id = if height == GENESIS_HEIGHT {
        None
    } else {
        Some(celestia_tendermint::block::Id {
            hash: Hash::Sha256(rand::random()),
            part_set_header: parts::Header::new(1, Hash::Sha256(rand::random()))
                .expect("invalid PartSetHeader"),
        })
    };

    let mut header = ExtendedHeader {
        header: Header {
            version: Version {
                block: version::BLOCK_PROTOCOL,
                app: 1,
            },
            chain_id: chain_id.clone(),
            height: height.try_into().unwrap(),
            time: Time::now(),
            last_block_id,
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
            height: height.try_into().unwrap(),
            round: 0_u16.into(),
            block_id: celestia_tendermint::block::Id {
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
        validator_set: ValidatorSet::new(
            vec![celestia_tendermint::validator::Info {
                address: validator_address,
                pub_key,
                power: 5000_u32.into(),
                name: None,
                proposer_priority: 0_i64.into(),
            }],
            Some(celestia_tendermint::validator::Info {
                address: validator_address,
                pub_key,
                power: 5000_u32.into(),
                name: None,
                proposer_priority: 0_i64.into(),
            }),
        ),
        dah: dah.unwrap_or_else(|| DataAvailabilityHeader::from_eds(&ExtendedDataSquare::empty())),
    };

    hash_and_sign(&mut header, signing_key);
    header.validate().expect("invalid header generated");

    header
}

fn generate_next(
    increment: u64,
    current: &ExtendedHeader,
    signing_key: &SigningKey,
    dah: Option<DataAvailabilityHeader>,
) -> ExtendedHeader {
    assert!(increment > 0);

    let validator_address = current.validator_set.validators()[0].address;

    let height = (current.header.height.value() + increment)
        .try_into()
        .unwrap();

    let last_block_id = if increment == 1 {
        Some(current.commit.block_id)
    } else {
        Some(celestia_tendermint::block::Id {
            hash: Hash::Sha256(rand::random()),
            part_set_header: parts::Header::new(1, Hash::Sha256(rand::random()))
                .expect("invalid PartSetHeader"),
        })
    };

    let mut header = ExtendedHeader {
        header: Header {
            version: current.header.version,
            chain_id: current.header.chain_id.clone(),
            height,
            time: Time::now(),
            last_block_id,
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
            height,
            round: 0_u16.into(),
            block_id: celestia_tendermint::block::Id {
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
        dah: dah.unwrap_or_else(|| DataAvailabilityHeader::from_eds(&ExtendedDataSquare::empty())),
    };

    hash_and_sign(&mut header, signing_key);
    header.validate().expect("invalid header generated");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

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
        genesis.verify(&header5).unwrap();
    }

    #[test]
    fn new_and_skip() {
        let mut gen = ExtendedHeaderGenerator::new();

        gen.skip(3);
        let header4 = gen.next();
        let header5 = gen.next();

        assert_eq!(header4.height().value(), 4);
        assert_eq!(header5.height().value(), 5);
        header4.verify(&header5).unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        gen.skip(1);
        let header2 = gen.next();
        let header3 = gen.next();

        assert_eq!(header2.height().value(), 2);
        assert_eq!(header3.height().value(), 3);
        header2.verify(&header3).unwrap();

        let mut gen = ExtendedHeaderGenerator::new();

        gen.skip(0);
        let genesis = gen.next();
        let header2 = gen.next();

        assert_eq!(genesis.height().value(), 1);
        assert_eq!(header2.height().value(), 2);
        genesis.verify(&header2).unwrap();
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

    #[test]
    fn invalidate_header() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let mut header6 = gen.next();
        let mut header7 = gen.next();

        invalidate(&mut header6);

        // Check that can be called multiple times
        invalidate(&mut header7);
        invalidate(&mut header7);
        invalidate(&mut header7);

        header6.validate().unwrap_err();
        header5.verify(&header6).unwrap();

        header7.validate().unwrap_err();
        header5.verify(&header7).unwrap();
    }

    #[test]
    fn unverify_header() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let mut header6 = gen.next();
        let mut header7 = gen.next();

        unverify(&mut header6);

        // Check that can be called multiple times
        unverify(&mut header7);
        unverify(&mut header7);
        unverify(&mut header7);

        header6.validate().unwrap();
        header5.verify(&header6).unwrap_err();

        header7.validate().unwrap();
        header5.verify(&header7).unwrap_err();
    }

    #[test]
    fn invalidate_and_unverify_header() {
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let mut header6 = gen.next();

        invalidate(&mut header6);
        unverify(&mut header6);

        header6.validate().unwrap_err();
        header5.verify(&header6).unwrap_err();

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        let header5 = gen.next();
        let mut header6 = gen.next();

        // check different order too
        unverify(&mut header6);
        invalidate(&mut header6);

        header6.validate().unwrap_err();
        header5.verify(&header6).unwrap_err();
    }
}
