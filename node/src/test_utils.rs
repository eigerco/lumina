use celestia_types::test_utils::ExtendedHeaderGenerator;

use crate::store::InMemoryStore;

pub fn gen_filled_store(amount: u64) -> (InMemoryStore, ExtendedHeaderGenerator) {
    let s = InMemoryStore::new();
    let mut gen = ExtendedHeaderGenerator::new();

    let headers = gen.next_many(amount);

    for header in headers {
        s.append_single_unchecked(header)
            .expect("inserting test data failed");
    }

    (s, gen)
}
