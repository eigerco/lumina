use celestia_types::nmt::{Namespace, NS_ID_V0_SIZE};
use rand::{Rng, RngCore};

pub mod client;
#[cfg(feature = "libp2p")]
pub mod tiny_node;

fn ns_to_u128(ns: Namespace) -> u128 {
    let mut bytes = [0u8; 16];
    let id = ns.id_v0().unwrap();
    bytes[6..].copy_from_slice(id);
    u128::from_be_bytes(bytes)
}

pub fn random_ns() -> Namespace {
    Namespace::const_v0(random_bytes_array())
}

pub fn random_ns_range(start: Namespace, end: Namespace) -> Namespace {
    let start = ns_to_u128(start);
    let end = ns_to_u128(end);

    let num = rand::thread_rng().gen_range(start..end);
    let bytes = num.to_be_bytes();
    let id = &bytes[bytes.len() - NS_ID_V0_SIZE..];

    Namespace::new_v0(id).unwrap()
}

pub fn random_bytes(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

pub fn random_bytes_array<const N: usize>() -> [u8; N] {
    std::array::from_fn(|_| rand::random())
}
