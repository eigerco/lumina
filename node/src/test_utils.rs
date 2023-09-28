use celestia_types::test_utils::{generate_ed25519_key, SigningKey};
use celestia_types::ExtendedHeader;

use crate::store::InMemoryStore;

pub fn gen_filled_store(height: u64) -> (InMemoryStore, SigningKey) {
    let s = InMemoryStore::new();

    let (headers, key) = gen_until_height(height);

    for header in headers {
        s.append_single_unverified(header)
            .expect("inserting test data failed");
    }

    (s, key)
}

pub fn gen_next_amount(
    header: &ExtendedHeader,
    amount: usize,
    key: &SigningKey,
) -> Vec<ExtendedHeader> {
    let mut headers = Vec::new();

    for _ in 0..amount {
        let header = headers.last().unwrap_or(header);
        headers.push(header.generate_next(key));
    }

    headers
}

pub fn gen_height(height: u64) -> (ExtendedHeader, SigningKey) {
    assert!(height > 0);

    let key = generate_ed25519_key();
    let mut header = ExtendedHeader::generate_genesis("private", &key);

    for _ in 2..=height {
        header = header.generate_next(&key);
    }

    (header, key)
}

pub fn gen_until_height(height: u64) -> (Vec<ExtendedHeader>, SigningKey) {
    assert!(height > 0);

    let key = generate_ed25519_key();
    let mut headers = vec![ExtendedHeader::generate_genesis("private", &key)];

    for _ in 2..=height {
        let header = headers.last().unwrap().generate_next(&key);
        headers.push(header)
    }

    (headers, key)
}
