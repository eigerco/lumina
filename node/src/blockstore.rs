//! Blockstore types aliases with lumina specific constants.

use crate::p2p::MAX_MH_SIZE;

/// An [`InMemoryBlockstore`] with maximum multihash size used by lumina.
///
/// [`InMemoryBlockstore`]: blockstore::InMemoryBlockstore
pub type InMemoryBlockstore = blockstore::InMemoryBlockstore<MAX_MH_SIZE>;

#[cfg(not(target_arch = "wasm32"))]
/// A [`SledBlockstore`] with maximum multihash size used by lumina.
///
/// [`SledBlockstore`]: blockstore::SledBlockstore
pub type SledBlockstore = blockstore::SledBlockstore<MAX_MH_SIZE>;

#[cfg(target_arch = "wasm32")]
/// A [`IndexedDbBlockstore`] with maximum multihash size used by lumina.
///
/// [`IndexedDbBlockstore`]: blockstore::IndexedDbBlockstore
pub type IndexedDbBlockstore = blockstore::IndexedDbBlockstore<MAX_MH_SIZE>;
