use crate::nmt::NS_SIZE;
use crate::Result;

const NAMESPACED_HASH_SIZE: usize = NamespacedHash::size();

pub type NamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;
pub type RawNamespacedHash = [u8; NAMESPACED_HASH_SIZE];

pub trait NamespacedHashExt {
    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash>;
    fn to_vec(&self) -> Vec<u8>;
    fn to_array(&self) -> RawNamespacedHash;
}

impl NamespacedHashExt for NamespacedHash {
    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash> {
        Ok(bytes.try_into()?)
    }

    fn to_vec(&self) -> Vec<u8> {
        self.iter().collect()
    }

    fn to_array(&self) -> RawNamespacedHash {
        let mut out = [0; NAMESPACED_HASH_SIZE];
        out[..NS_SIZE].copy_from_slice(&self.min_namespace().0);
        out[NS_SIZE..2 * NS_SIZE].copy_from_slice(&self.max_namespace().0);
        out[2 * NS_SIZE..].copy_from_slice(&self.hash());
        out
    }
}
