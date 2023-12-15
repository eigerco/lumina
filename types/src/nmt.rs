use base64::prelude::*;
use blockstore::multihash::CidError;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::simple_merkle::db::MemDb;
use nmt_rs::simple_merkle::tree::MerkleHash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tendermint::hash::SHA256_HASH_SIZE;
use tendermint_proto::serializers::cow_str::CowStr;

mod namespace_proof;
mod namespaced_hash;

pub use self::namespace_proof::NamespaceProof;
pub use self::namespaced_hash::{
    NamespacedHashExt, RawNamespacedHash, HASH_SIZE, NAMESPACED_HASH_SIZE,
};
use crate::{Error, Result};

pub const NS_VER_SIZE: usize = 1;
pub const NS_ID_SIZE: usize = 28;
pub const NS_SIZE: usize = NS_VER_SIZE + NS_ID_SIZE;
pub const NS_ID_V0_SIZE: usize = 10;

pub const NMT_MULTIHASH_CODE: u64 = 0x7700;
pub const NMT_CODEC: u64 = 0x7701;
pub const NMT_ID_SIZE: usize = 2 * NS_SIZE + SHA256_HASH_SIZE;

pub type NamespacedSha2Hasher = nmt_rs::NamespacedSha2Hasher<NS_SIZE>;
pub type NamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;
pub type Nmt = nmt_rs::NamespaceMerkleTree<MemDb<NamespacedHash>, NamespacedSha2Hasher, NS_SIZE>;

pub struct NodePair(NamespacedHash, NamespacedHash);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct Namespace(nmt_rs::NamespaceId<NS_SIZE>);

impl Namespace {
    pub const MAX: Namespace = Namespace(nmt_rs::NamespaceId::MAX_ID);

    pub fn from_raw(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != NS_SIZE {
            return Err(Error::InvalidNamespaceSize);
        }

        Namespace::new(bytes[0], &bytes[1..])
    }

    pub fn new(version: u8, id: &[u8]) -> Result<Self> {
        match version {
            0 => Self::new_v0(id),
            255 => Self::try_max(id),
            n => Err(Error::UnsupportedNamespaceVersion(n)),
        }
    }

    pub fn new_v0(id: &[u8]) -> Result<Self> {
        let id_pos = match id.len() {
            // Allow 28 bytes len
            NS_ID_SIZE => NS_ID_SIZE - NS_ID_V0_SIZE,
            // Allow 10 bytes len or less
            n if n <= NS_ID_V0_SIZE => 0,
            // Anything else is an error
            _ => return Err(Error::InvalidNamespaceSize),
        };

        let prefix = &id[..id_pos];
        let id = &id[id_pos..];

        // Validate that prefix is all zeros
        if prefix.iter().any(|&x| x != 0) {
            return Err(Error::InvalidNamespaceV0);
        }

        let mut bytes = [0u8; NS_SIZE];
        bytes[NS_SIZE - id.len()..].copy_from_slice(id);

        Ok(Namespace(nmt_rs::NamespaceId(bytes)))
    }

    pub(crate) const fn new_unchecked(bytes: [u8; NS_SIZE]) -> Self {
        Namespace(nmt_rs::NamespaceId(bytes))
    }

    pub const fn const_v0(id: [u8; NS_ID_V0_SIZE]) -> Self {
        let mut bytes = [0u8; NS_SIZE];
        let start = NS_SIZE - NS_ID_V0_SIZE;

        bytes[start] = id[0];
        bytes[start + 1] = id[1];
        bytes[start + 2] = id[2];
        bytes[start + 3] = id[3];
        bytes[start + 4] = id[4];
        bytes[start + 5] = id[5];
        bytes[start + 6] = id[6];
        bytes[start + 7] = id[7];
        bytes[start + 8] = id[8];
        bytes[start + 9] = id[9];

        Namespace(nmt_rs::NamespaceId(bytes))
    }

    fn try_max(id: &[u8]) -> Result<Self> {
        if id.iter().all(|&x| x == 0xff) {
            Ok(Namespace::MAX)
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

    pub fn id_v0(&self) -> Option<&[u8]> {
        if self.version() == 0 {
            let start = NS_SIZE - NS_ID_V0_SIZE;
            Some(&self.as_bytes()[start..])
        } else {
            None
        }
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

impl std::ops::Deref for Namespace {
    type Target = nmt_rs::NamespaceId<NS_SIZE>;

    fn deref(&self) -> &Self::Target {
        &self.0
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
        // base64 needs more buffer size than the final output
        let mut buf = [0u8; NS_SIZE * 2];

        let s = CowStr::deserialize(deserializer)?;

        let len = BASE64_STANDARD
            .decode_slice(s, &mut buf)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        Namespace::from_raw(&buf[..len]).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl NodePair {
    fn validate_namespace_order(&self) -> Result<()> {
        let NodePair(left, right) = self;

        left.validate_namespace_order()?;
        right.validate_namespace_order()?;

        if left.max_namespace() > right.min_namespace() {
            return Err(Error::InvalidNmtNodeOrder);
        }

        Ok(())
    }
}

impl TryFrom<NodePair> for CidGeneric<NMT_ID_SIZE> {
    type Error = CidError;

    fn try_from(nodes: NodePair) -> Result<Self, Self::Error> {
        nodes
            .validate_namespace_order()
            .map_err(|e| CidError::InvalidDataFormat(e.to_string()))?;

        let hasher = NamespacedSha2Hasher::with_ignore_max_ns(true);
        let digest = hasher.hash_nodes(&nodes.0, &nodes.1).to_array();

        let mh = Multihash::wrap(NMT_MULTIHASH_CODE, &digest).unwrap();

        Ok(CidGeneric::new_v1(NMT_CODEC, mh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn namespace_id_8_bytes() {
        let nid = Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();

        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            0, 0, 1, 2, 3, 4, 5, 6, 7, 8, // id with left padding
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_8_bytes_with_prefix() {
        let nid = Namespace::new_v0(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            0, 0, 1, 2, 3, 4, 5, 6, 7, 8, // id
        ])
        .unwrap();

        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            0, 0, 1, 2, 3, 4, 5, 6, 7, 8, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_10_bytes() {
        let nid = Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]).unwrap();
        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_const_v0() {
        let nid = Namespace::const_v0([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_10_bytes_with_prefix() {
        let nid = Namespace::new_v0(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        ])
        .unwrap();

        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_with_invalid_prefix() {
        let e = Namespace::new_v0(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        ])
        .unwrap_err();

        assert!(matches!(e, Error::InvalidNamespaceV0));
    }

    #[test]
    fn namespace_id_11_bytes() {
        let e = Namespace::new_v0(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]).unwrap_err();

        assert!(matches!(e, Error::InvalidNamespaceSize));
    }

    #[test]
    fn namespace_id_from_raw_bytes() {
        let nid = Namespace::from_raw(&[
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ])
        .unwrap();

        let expected_nid = Namespace(nmt_rs::NamespaceId([
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ]));

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn namespace_id_with_28_raw_bytes() {
        let e = Namespace::from_raw(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ])
        .unwrap_err();

        assert!(matches!(e, Error::InvalidNamespaceSize));
    }

    #[test]
    fn namespace_id_with_30_raw_bytes() {
        let e = Namespace::from_raw(&[
            0, // extra
            0, // version
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // prefix
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, // id
        ])
        .unwrap_err();

        assert!(matches!(e, Error::InvalidNamespaceSize));
    }

    #[test]
    fn max_namespace_id_from_raw_bytes() {
        let nid = Namespace::from_raw(&[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff,
        ])
        .unwrap();

        let expected_nid = Namespace::MAX;

        assert_eq!(nid, expected_nid);
    }

    #[test]
    fn invalid_max_namespace_id_from_raw_bytes() {
        let e = Namespace::from_raw(&[
            0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0,
        ])
        .unwrap_err();

        assert!(matches!(e, Error::UnsupportedNamespaceVersion(255)));
    }

    #[test]
    fn invalid_version() {
        let e = Namespace::from_raw(&[
            254, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap_err();

        assert!(matches!(e, Error::UnsupportedNamespaceVersion(254)));
    }

    #[test]
    fn test_generate_inner_multihash() {
        let ns0 = Namespace::new_v0(&[1]).unwrap();
        let ns1 = Namespace::new_v0(&[2]).unwrap();
        let ns2 = Namespace::new_v0(&[3]).unwrap();

        let nodes = NodePair(
            NamespacedHash::with_min_and_max_ns(*ns0, *ns1),
            NamespacedHash::with_min_and_max_ns(*ns1, *ns2),
        );

        let cid = CidGeneric::try_from(nodes).unwrap();
        assert_eq!(cid.codec(), NMT_CODEC);

        let hash = cid.hash();
        assert_eq!(hash.code(), NMT_MULTIHASH_CODE);
        assert_eq!(hash.size(), NAMESPACED_HASH_SIZE as u8);

        let hash = NamespacedHash::from_raw(hash.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *ns0);
        assert_eq!(hash.max_namespace(), *ns2);
    }

    #[test]
    fn invalid_ns_order_result() {
        let ns0 = Namespace::new_v0(&[1]).unwrap();
        let ns1 = Namespace::new_v0(&[2]).unwrap();
        let ns2 = Namespace::new_v0(&[3]).unwrap();

        let nodes = NodePair(
            NamespacedHash::with_min_and_max_ns(*ns1, *ns2),
            NamespacedHash::with_min_and_max_ns(*ns0, *ns0),
        );
        let result = CidGeneric::try_from(nodes).unwrap_err();

        assert_eq!(
            result,
            CidError::InvalidDataFormat("Invalid nmt node order".to_string())
        );
    }

    #[test]
    fn test_read_multihash() {
        let multihash = [
            0x81, 0xEE, 0x01, // code = 7701
            0x5A, // len = NAMESPACED_HASH_SIZE = 90
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // min ns
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            9, // max ns
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
        ];

        let mh = Multihash::<NAMESPACED_HASH_SIZE>::from_bytes(&multihash).unwrap();
        assert_eq!(mh.code(), NMT_CODEC);
        assert_eq!(mh.size(), NAMESPACED_HASH_SIZE as u8);
        let hash = NamespacedHash::from_raw(mh.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *Namespace::new_v0(&[1]).unwrap());
        assert_eq!(hash.max_namespace(), *Namespace::new_v0(&[9]).unwrap());
        assert_eq!(hash.hash(), [0xFF; 32]);
    }
}
