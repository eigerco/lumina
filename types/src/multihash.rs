use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::simple_merkle::tree::MerkleHash;
use nmt_rs::NamespaceMerkleHasher;
use tendermint::hash::SHA256_HASH_SIZE;

use crate::nmt::{Namespace, NamespacedHash, NamespacedHashExt, NamespacedSha2Hasher, NS_SIZE};
use crate::{Error, Result};

pub const MULTIHASH_NMT_CODEC_CODE: u64 = 0x7700;
pub const MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE: u64 = 0x7701;
pub const MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE: usize = 2 * NS_SIZE + SHA256_HASH_SIZE;

pub trait HasMultihash<const S: usize> {
    fn multihash(&self) -> Result<Multihash<S>>;
}

pub trait HasCid<const S: usize>: HasMultihash<S> {
    fn cid_v1(&self) -> Result<CidGeneric<S>> {
        Ok(CidGeneric::<S>::new_v1(Self::codec(), self.multihash()?))
    }

    fn codec() -> u64;
}

impl HasMultihash<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE> for (&Namespace, &[u8]) {
    fn multihash(&self) -> Result<Multihash<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE>> {
        let (ns, data) = self;
        let hasher = NamespacedSha2Hasher::with_ignore_max_ns(true);
        let namespaced_data = [ns.as_bytes(), data].concat();

        let digest = hasher.hash_leaf(&namespaced_data).to_array();
        // size is correct, so unwrap is safe
        Ok(Multihash::wrap(MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE, &digest).unwrap())
    }
}

impl HasCid<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE> for (&Namespace, &[u8]) {
    fn codec() -> u64 {
        MULTIHASH_NMT_CODEC_CODE
    }
}

impl HasMultihash<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE> for (&NamespacedHash, &NamespacedHash) {
    fn multihash(&self) -> Result<Multihash<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE>> {
        let (left, right) = self;
        left.validate_namespace_order()?;
        right.validate_namespace_order()?;

        if left.max_namespace() > right.min_namespace() {
            return Err(Error::InvalidNmtNodeOrder);
        }

        let hasher = NamespacedSha2Hasher::with_ignore_max_ns(true);

        let digest = hasher.hash_nodes(left, right).to_array();

        Ok(Multihash::wrap(MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE, &digest).unwrap())
    }
}

impl HasCid<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE> for (&NamespacedHash, &NamespacedHash) {
    fn codec() -> u64 {
        MULTIHASH_NMT_CODEC_CODE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::{Namespace, NAMESPACED_HASH_SIZE};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn test_generate_leaf_multihash() {
        let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
        let data = [0xCDu8; 512];

        let hash = (&namespace, data.as_ref()).multihash().unwrap();

        assert_eq!(hash.code(), MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE);
        assert_eq!(hash.size(), NAMESPACED_HASH_SIZE as u8);
        let hash = NamespacedHash::from_raw(hash.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *namespace);
        assert_eq!(hash.max_namespace(), *namespace);
    }

    #[test]
    fn test_generate_inner_multihash() {
        let ns0 = Namespace::new_v0(&[1]).unwrap();
        let ns1 = Namespace::new_v0(&[2]).unwrap();
        let ns2 = Namespace::new_v0(&[3]).unwrap();

        let left = NamespacedHash::with_min_and_max_ns(*ns0, *ns1);
        let right = NamespacedHash::with_min_and_max_ns(*ns1, *ns2);

        let hash = (&left, &right).multihash().unwrap();

        assert_eq!(hash.code(), MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE);
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

        let left = NamespacedHash::with_min_and_max_ns(*ns1, *ns2);
        let right = NamespacedHash::with_min_and_max_ns(*ns0, *ns0);

        //let multihash = Code::Sha256NamespaceFlagged;
        //let digest_result = multihash.digest_nodes(&left, &right).unwrap_err();
        let result = (&left, &right).multihash().unwrap_err();
        assert!(matches!(result, Error::InvalidNmtNodeOrder));
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
        assert_eq!(mh.code(), MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE);
        assert_eq!(mh.size(), NAMESPACED_HASH_SIZE as u8);
        let hash = NamespacedHash::from_raw(mh.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *Namespace::new_v0(&[1]).unwrap());
        assert_eq!(hash.max_namespace(), *Namespace::new_v0(&[9]).unwrap());
        assert_eq!(hash.hash(), [0xFF; 32]);
    }
}
