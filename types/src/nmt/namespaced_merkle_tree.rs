use nmt_rs::{simple_merkle::db::MemDb, NamespaceMerkleHasher};

use super::{NamespacedHash, NS_SIZE};

pub use nmt_rs::simple_merkle::tree::MerkleHash;

/// Hasher for generating [`Namespace`] aware hashes.
///
/// [`Namespace`] crate::nmt::Namespace
pub type NamespacedSha2Hasher = nmt_rs::NamespacedSha2Hasher<NS_SIZE>;
/// [`Namespace`] aware merkle tree.
///
/// [`Namespace`] crate::nmt::Namespace
pub type Nmt = nmt_rs::NamespaceMerkleTree<MemDb<NamespacedHash>, NamespacedSha2Hasher, NS_SIZE>;

/// An extention trait for the [`NamespacedHash`] to perform additional actions.
pub trait NmtExt {
    /// Create a new [`Nmt`] with default hasher used in Celestia.
    fn default() -> Nmt;
}

impl NmtExt for Nmt {
    fn default() -> Nmt {
        Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true))
    }
}
