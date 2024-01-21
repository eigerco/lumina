use crate::nmt::{NamespacedHash, NamespacedSha2Hasher, NS_SIZE};
use crate::types::Vec;
use crate::{Error, Result};

use nmt_rs::simple_merkle::tree::MerkleHash;

/// Size of the [`NamespacedHash`] in bytes.
pub const NAMESPACED_HASH_SIZE: usize = NamespacedHash::size();
/// Size of the Sha256 hash in [`NamespacedHash`] in bytes.
pub const HASH_SIZE: usize = 32;

/// Byte representation of the [`NamespacedHash`].
pub type RawNamespacedHash = [u8; NAMESPACED_HASH_SIZE];

/// An extention trait for the [`NamespacedHash`] to perform additional actions.
pub trait NamespacedHashExt {
    /// Get the hash of the root of an empty [`Nmt`].
    ///
    /// [`Nmt`]: crate::nmt::Nmt
    fn empty_root() -> NamespacedHash;
    /// Try to decode [`NamespacedHash`] from the raw bytes.
    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash>;
    /// Encode [`NamespacedHash`] into [`Vec`].
    fn to_vec(&self) -> Vec<u8>;
    /// Encode [`NamespacedHash`] into [`array`].
    fn to_array(&self) -> RawNamespacedHash;
    /// Validate if the [`Namespace`]s covered by this hash are in correct order.
    ///
    /// I.e. this verifies that the minimum [`Namespace`] of this hash is lower
    /// or equal to the maximum [`Namespace`].
    ///
    /// [`Namespace`]: crate::nmt::Namespace
    fn validate_namespace_order(&self) -> Result<()>;
}

impl NamespacedHashExt for NamespacedHash {
    fn empty_root() -> NamespacedHash {
        NamespacedSha2Hasher::EMPTY_ROOT
    }

    fn from_raw(bytes: &[u8]) -> Result<NamespacedHash> {
        Ok(bytes
            .try_into()
            .map_err(|hash| Error::InvalidNamespacedHash(hash))?)
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

    fn validate_namespace_order(&self) -> Result<()> {
        if self.min_namespace() > self.max_namespace() {
            return Err(Error::InvalidNmtNodeOrder);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::{Namespace, NS_ID_V0_SIZE};

    #[test]
    fn namespaced_hash_validate_namespace_order() {
        let n0 = Namespace::new_v0(&[1]).unwrap();
        let n1 = Namespace::new_v0(&[2]).unwrap();

        assert!(NamespacedHash::with_min_and_max_ns(*n0, *n1)
            .validate_namespace_order()
            .is_ok());
        assert!(NamespacedHash::with_min_and_max_ns(*n1, *n1)
            .validate_namespace_order()
            .is_ok());
        assert!(NamespacedHash::with_min_and_max_ns(*n1, *n0)
            .validate_namespace_order()
            .is_err());
    }

    #[test]
    fn hash_to_array() {
        let ns_min = [9; NS_ID_V0_SIZE];
        let ns_max = [2; NS_ID_V0_SIZE];

        let mut ns_bytes_min = [0; NS_SIZE];
        ns_bytes_min[NS_SIZE - NS_ID_V0_SIZE..].copy_from_slice(&ns_min);
        let mut ns_bytes_max = [0; NS_SIZE];
        ns_bytes_max[NS_SIZE - NS_ID_V0_SIZE..].copy_from_slice(&ns_max);

        let buff = NamespacedHash::with_min_and_max_ns(
            *Namespace::new_v0(&ns_min).unwrap(),
            *Namespace::new_v0(&ns_max).unwrap(),
        )
        .to_array();

        assert_eq!(buff[..NS_SIZE], ns_bytes_min);
        assert_eq!(buff[NS_SIZE..NS_SIZE * 2], ns_bytes_max);
        assert_eq!(buff[NS_SIZE * 2..], [0; HASH_SIZE]);
    }
}
