use celestia_proto::header::pb::ExtendedHeader as RawExtendedHeader;
use celestia_types::ExtendedHeader;
use celestia_types::{DataAvailabilityHeader, ValidatorSet};
use tendermint::block::header::Header;
use tendermint::block::Commit;
use tendermint::Hash;
use tendermint::Time;
use tendermint::{block::header::Version, AppHash};

use crate::store::InMemoryStore;

pub fn gen_extended_header(height: u64) -> ExtendedHeader {
    RawExtendedHeader {
        header: Some(
            Header {
                version: Version { block: 11, app: 1 },
                chain_id: "private".to_string().try_into().unwrap(),
                height: height.try_into().unwrap(),
                time: Time::now(),
                last_block_id: None,
                last_commit_hash: Hash::default(),
                data_hash: Hash::default(),
                validators_hash: Hash::default(),
                next_validators_hash: Hash::default(),
                consensus_hash: Hash::default(),
                app_hash: AppHash::default(),
                last_results_hash: Hash::default(),
                evidence_hash: Hash::default(),
                proposer_address: tendermint::account::Id::new([0; 20]),
            }
            .into(),
        ),
        commit: Some(
            Commit {
                height: height.try_into().unwrap(),
                block_id: tendermint::block::Id {
                    hash: Hash::Sha256(rand::random()),
                    ..Default::default()
                },
                ..Default::default()
            }
            .into(),
        ),
        validator_set: Some(ValidatorSet::new(Vec::new(), None).into()),
        dah: Some(
            DataAvailabilityHeader {
                row_roots: Vec::new(),
                column_roots: Vec::new(),
                hash: [0; 32],
            }
            .into(),
        ),
    }
    .try_into()
    .unwrap()
}

pub fn gen_filled_store(height: u64) -> InMemoryStore {
    let s = InMemoryStore::new();

    // block height is 1-indexed
    for height in 1..=height {
        s.append_continuous(gen_extended_header(height))
            .expect("inserting test data failed");
    }

    s
}
