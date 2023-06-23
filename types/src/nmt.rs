use base64::prelude::*;
use nmt_rs::simple_merkle::db::MemDb;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result};

pub const NS_VER_SIZE: usize = 1;
pub const NS_ID_SIZE: usize = 28;
pub const NS_SIZE: usize = NS_VER_SIZE + NS_ID_SIZE;
pub const NS_ID_V0_MAX_SIZE: usize = 10;

pub type NamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;
pub type NamespacedSha2Hasher = nmt_rs::NamespacedSha2Hasher<NS_SIZE>;
pub type NamespaceProof = nmt_rs::nmt_proof::NamespaceProof<NamespacedSha2Hasher, NS_SIZE>;
pub type Nmt = nmt_rs::NamespaceMerkleTree<MemDb<NamespacedHash>, NamespacedSha2Hasher, NS_SIZE>;

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub struct Namespace(nmt_rs::NamespaceId<NS_SIZE>);

impl Namespace {
    pub fn from_raw(bytes: &[u8]) -> Result<Self> {
        if bytes.len() <= NS_VER_SIZE {
            return Err(Error::InvalidNamespaceIdSize(bytes.len()));
        }

        Namespace::new(bytes[0], &bytes[1..])
    }

    pub fn new(version: u8, id: &[u8]) -> Result<Self> {
        match version {
            0 => Self::new_v0(id),
            255 => Self::new_max(id),
            n => Err(Error::UnsupportedNamespaceVersion(n)),
        }
    }

    pub fn new_v0(id: &[u8]) -> Result<Self> {
        let start_pos = id.iter().position(|&x| x != 0).unwrap_or(0);
        let id = &id[start_pos..];

        if id.len() > NS_ID_V0_MAX_SIZE {
            return Err(Error::InvalidNamespaceIdSize(id.len()));
        }

        let mut bytes = [0u8; NS_SIZE];
        bytes[NS_SIZE - id.len()..].copy_from_slice(id);

        Ok(Namespace(nmt_rs::NamespaceId(bytes)))
    }

    pub fn new_max(id: &[u8]) -> Result<Self> {
        if id.iter().all(|&x| x == 0xff) {
            Ok(Namespace(nmt_rs::NamespaceId::max_id()))
        } else {
            Err(Error::UnsupportedNamespaceVersion(255))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0 .0
    }

    pub fn version(&self) -> u8 {
        self.as_bytes()[0]
    }

    pub fn id(&self) -> &[u8] {
        &self.as_bytes()[1..]
    }
}

impl From<Namespace> for nmt_rs::NamespaceId<NS_SIZE> {
    fn from(value: Namespace) -> Self {
        value.0
    }
}

impl From<nmt_rs::NamespaceId<NS_SIZE>> for Namespace {
    fn from(value: nmt_rs::NamespaceId<NS_SIZE>) -> Self {
        Namespace(value)
    }
}

impl Serialize for Namespace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = BASE64_STANDARD.encode(self.0);
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for Namespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let bytes = BASE64_STANDARD
            .decode(s)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        nmt_rs::NamespaceId::try_from(&bytes[..])
            .map(Namespace)
            .map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_id_8_bytes() {
        let nid = Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, // reserved filled with zeros
            0, 0, 1, 2, 3, 4, 5, 6, 7, 8, // id with left padding
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_10_bytes() {
        let nid = Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]).unwrap();
        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, // reserved filled with zeros
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_11_bytes() {
        Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]).unwrap_err();
    }
}
