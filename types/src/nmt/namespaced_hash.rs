use std::ops::{Deref, DerefMut};

use crate::nmt::NS_SIZE;
use crate::{Error, Result};

pub(crate) type NmtNamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacedHash(NmtNamespacedHash);

impl NamespacedHash {
    pub fn from_raw(bytes: &[u8]) -> Result<Self> {
        let hash = to_nmt_namespaced_hash(bytes)?;
        Ok(NamespacedHash(hash))
    }

    pub fn into_inner(self) -> NmtNamespacedHash {
        self.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.iter().copied().collect()
    }
}

impl Deref for NamespacedHash {
    type Target = NmtNamespacedHash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NamespacedHash {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<NamespacedHash> for NmtNamespacedHash {
    fn from(value: NamespacedHash) -> NmtNamespacedHash {
        value.0
    }
}

impl From<NmtNamespacedHash> for NamespacedHash {
    fn from(value: NmtNamespacedHash) -> Self {
        NamespacedHash(value)
    }
}

impl TryFrom<&[u8]> for NamespacedHash {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        NamespacedHash::from_raw(value)
    }
}

impl TryFrom<Vec<u8>> for NamespacedHash {
    type Error = Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        NamespacedHash::from_raw(&value)
    }
}

impl TryFrom<&'_ Vec<u8>> for NamespacedHash {
    type Error = Error;

    fn try_from(value: &'_ Vec<u8>) -> Result<Self, Self::Error> {
        NamespacedHash::from_raw(value)
    }
}

pub(crate) fn to_nmt_namespaced_hash(bytes: &[u8]) -> Result<NmtNamespacedHash> {
    bytes.try_into().map_err(|_| Error::InvalidNamespacedHash)
}
