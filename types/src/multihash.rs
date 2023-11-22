use multihash_derive::{Hasher, MultihashDigest};
use nmt_rs::simple_merkle::tree::MerkleHash;
use nmt_rs::NamespaceMerkleHasher;

use crate::byzantine::MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE;
use crate::nmt::{Namespace, NamespacedHashExt, NamespacedSha2Hasher, NS_SIZE, NAMESPACED_HASH_SIZE, HASH_SIZE};
use crate::{Error, Result};

pub type NamespacedHash = nmt_rs::NamespacedHash<NS_SIZE>;

struct Sha256NamespaceFlaggedHasher {
    hasher: NamespacedSha2Hasher,
    hash: Option<NamespacedHash>,
    hash_buf: [u8; NAMESPACED_HASH_SIZE],
}

impl Sha256NamespaceFlaggedHasher {
    fn hash_data(&mut self, data: &[u8]) -> Result<()> {
        let hash = if data.len() == NAMESPACED_HASH_SIZE * 2 {
            let (left, right) = data.split_at(NAMESPACED_HASH_SIZE);
            self.hasher
                .hash_nodes(&left.try_into()?, &right.try_into()?)
        } else {
            self.hasher.hash_leaf(data)
        };

        self.hash = Some(hash);

        Ok(())
    }
}

impl Hasher for Sha256NamespaceFlaggedHasher {
    fn update(&mut self, data: &[u8]) {
        self.hash_data(data).expect("invalid data to hash");
    }

    fn finalize(&mut self) -> &[u8] {
        let hash = if let Some(hash) = &self.hash {
            hash
        } else {
            &NamespacedSha2Hasher::EMPTY_ROOT
        };

        self.hash_buf = hash.to_array();

        &self.hash_buf
    }

    fn reset(&mut self) {
        self.hash = None;
    }
}

impl Default for Sha256NamespaceFlaggedHasher {
    fn default() -> Self {
        Self {
            hasher: NamespacedSha2Hasher::with_ignore_max_ns(true),
            hash: None,
            hash_buf: [0u8; NAMESPACED_HASH_SIZE],
        }
    }
}

/// Outputs Multihashes, may panic when running `digest`. See `MultihashDigestExt` trait methods
/// for safe alternative.
#[derive(Clone, Copy, Debug, Eq, MultihashDigest, PartialEq)]
#[mh(alloc_size = 128)]
pub enum Code {
    #[mh(code = MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE, hasher = Sha256NamespaceFlaggedHasher)]
    Sha256NamespaceFlagged,
}

trait MultihashDigestExt {
    fn digest_leaf(&self, ns: &Namespace, data: &[u8]) -> Result<Multihash>;
    fn digest_inner(&self, left: &NamespacedHash, right: &NamespacedHash) -> Result<Multihash>;
}

impl MultihashDigestExt for Code {
    fn digest_leaf(&self, ns: &Namespace, data: &[u8]) -> Result<Multihash> {
        let namespaced_data = [ns.as_bytes(), data].concat();

        Ok(self.digest(&namespaced_data))
    }

    fn digest_inner(&self, left: &NamespacedHash, right: &NamespacedHash) -> Result<Multihash> {
        left.validate_namespace_order()?;
        right.validate_namespace_order()?;

        if left.max_namespace() > right.min_namespace() {
            return Err(Error::InvalidNmtNodeOrder);
        }

        let mut buffer = [0u8; NAMESPACED_HASH_SIZE * 2];
        buffer[..NAMESPACED_HASH_SIZE].copy_from_slice(&left.to_array());
        buffer[NAMESPACED_HASH_SIZE..].copy_from_slice(&right.to_array());

        Ok(self.digest(&buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::Namespace;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn test_generate_leaf_multihash() {
        let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
        let mut data = [0xCD; 512];
        data[0..NS_SIZE].copy_from_slice(namespace.as_bytes());

        let multihash = Code::Sha256NamespaceFlagged;
        let hash = multihash.digest(&data);

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

        let mut data = left.to_vec();
        data.extend_from_slice(&right.to_array());

        let multihash = Code::Sha256NamespaceFlagged;
        let hash = multihash.digest(&data);
        let hash0 = multihash.digest_inner(&left, &right).unwrap();
        assert_eq!(hash, hash0);

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

        let multihash = Code::Sha256NamespaceFlagged;
        let digest_result = multihash.digest_inner(&left, &right).unwrap_err();
        assert!(matches!(digest_result, Error::InvalidNmtNodeOrder));
    }

    #[test]
    #[should_panic]
    fn invalid_ns_order_panic() {
        let ns0 = Namespace::new_v0(&[1]).unwrap();
        let ns1 = Namespace::new_v0(&[2]).unwrap();
        let ns2 = Namespace::new_v0(&[3]).unwrap();

        let left = NamespacedHash::with_min_and_max_ns(*ns1, *ns2);
        let right = NamespacedHash::with_min_and_max_ns(*ns0, *ns0);

        let mut data = left.to_vec();
        data.extend_from_slice(&right.to_array());

        let multihash = Code::Sha256NamespaceFlagged;
        multihash.digest(&data);
    }

    #[test]
    #[should_panic]
    fn leaf_data_too_short() {
        let namespace = [0; NS_SIZE - 1];

        let multihash = Code::Sha256NamespaceFlagged;
        multihash.digest(&namespace);
    }

    #[test]
    fn test_read_multihash() {
        let multihash = [
            0x81, 0xee, 0x01, // code = 7701
            0x5a, // len = NAMESPACED_HASH_SIZE = 90
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, // min ns
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            9, // max ns
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, // hash
        ];

        let mh = Multihash::from_bytes(&multihash).unwrap();
        assert_eq!(mh.code(), MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE);
        assert_eq!(mh.size(), NAMESPACED_HASH_SIZE as u8);
        let hash = NamespacedHash::from_raw(mh.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *Namespace::new_v0(&[1]).unwrap());
        assert_eq!(hash.max_namespace(), *Namespace::new_v0(&[9]).unwrap());
        assert_eq!(hash.hash(), [0xFF; 32]);
    }
}
