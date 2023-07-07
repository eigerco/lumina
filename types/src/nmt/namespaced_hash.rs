use crate::nmt::NS_SIZE;
use crate::{Error, Result};

pub type NamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;

pub trait NamespacedHashExt {
    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash>;
    fn to_vec(&self) -> Vec<u8>;
}

impl NamespacedHashExt for NamespacedHash {
    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash> {
        bytes.try_into().map_err(|_| Error::InvalidNamespacedHash)
    }

    fn to_vec(&self) -> Vec<u8> {
        self.iter().copied().collect()
    }
}
