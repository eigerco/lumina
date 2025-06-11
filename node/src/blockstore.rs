//! Blockstore types aliases with lumina specific constants.

use blockstore::{Blockstore, Result};
use celestia_types::sample::SAMPLE_ID_CODEC;
use cid::CidGeneric;

use crate::p2p::MAX_MH_SIZE;

/// An [`InMemoryBlockstore`] with maximum multihash size used by lumina.
///
/// [`InMemoryBlockstore`]: blockstore::InMemoryBlockstore
pub type InMemoryBlockstore = SampleBlockstore<blockstore::InMemoryBlockstore<MAX_MH_SIZE>>;

#[cfg(not(target_arch = "wasm32"))]
/// A [`RedbBlockstore`].
///
/// [`RedbBlockstore`]: blockstore::RedbBlockstore
pub type RedbBlockstore = SampleBlockstore<blockstore::RedbBlockstore>;

#[cfg(target_arch = "wasm32")]
/// An [`IndexedDbBlockstore`].
///
/// [`IndexedDbBlockstore`]: blockstore::IndexedDbBlockstore
pub type IndexedDbBlockstore = SampleBlockstore<blockstore::IndexedDbBlockstore>;

/// A blockstore which only stores samples and discards other CIDs.
pub struct SampleBlockstore<B> {
    blockstore: B,
}

impl<B> SampleBlockstore<B> {
    /// Extract the underlying blockstore
    pub fn into_inner(self) -> B {
        self.blockstore
    }
}

impl SampleBlockstore<blockstore::InMemoryBlockstore<MAX_MH_SIZE>> {
    /// Create a new [`InMemoryBlockstore`]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            blockstore: blockstore::InMemoryBlockstore::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SampleBlockstore<blockstore::RedbBlockstore> {
    /// Create a new [`RedbBlockstore`]
    pub fn new(db: std::sync::Arc<redb::Database>) -> Self {
        Self {
            blockstore: blockstore::RedbBlockstore::new(db),
        }
    }

    /// Open a persistent [`RedbBlockstore`]
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Ok(Self {
            blockstore: blockstore::RedbBlockstore::open(path).await?,
        })
    }

    /// Open an in memory [`RedbBlockstore`]
    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            blockstore: blockstore::RedbBlockstore::in_memory()?,
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl SampleBlockstore<blockstore::IndexedDbBlockstore> {
    /// Create a new [`IndexedDbBlockstore`]
    pub async fn new(name: &str) -> Result<Self> {
        Ok(Self {
            blockstore: blockstore::IndexedDbBlockstore::new(name).await?,
        })
    }
}

impl<B> Blockstore for SampleBlockstore<B>
where
    B: Blockstore,
{
    async fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<Option<Vec<u8>>> {
        self.blockstore.get(cid).await
    }

    async fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> Result<()> {
        if cid.codec() == SAMPLE_ID_CODEC {
            self.blockstore.put_keyed(cid, data).await
        } else {
            Ok(())
        }
    }

    async fn remove<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<()> {
        self.blockstore.remove(cid).await
    }

    async fn has<const S: usize>(&self, cid: &CidGeneric<S>) -> Result<bool> {
        self.blockstore.has(cid).await
    }

    async fn close(self) -> Result<()> {
        self.blockstore.close().await
    }
}

#[cfg(test)]
mod tests {
    use blockstore::Blockstore;
    use celestia_types::{
        nmt::Namespace, row::RowId, row_namespace_data::RowNamespaceDataId, sample::SampleId,
    };
    use lumina_utils::test_utils::async_test;

    use super::InMemoryBlockstore;

    #[async_test]
    async fn should_only_store_samples() {
        macro_rules! cid {
            ($bytes:expr) => {
                ::cid::CidGeneric::try_from($bytes).unwrap()
            };
            ($id:ty, $($args:expr),+ $(,)?) => {
                $crate::p2p::shwap::convert_cid(
                    &<$id>::new($($args),+).unwrap().into()
                )
                .unwrap()
            };
        }

        let blockstore = InMemoryBlockstore::new();

        let sample_cids = [
            cid!(SampleId, 1, 2, 3),
            cid!(SampleId, 1111, 232, 33),
            cid!(SampleId, 123, 1, 888888888),
        ];

        let non_sample_cids = [
            cid!(RowId, 1, 1737),
            cid!(RowId, 8812, 193139),
            cid!(RowNamespaceDataId, Namespace::new_v0(b"a").unwrap(), 15, 12),
            cid!(RowNamespaceDataId, Namespace::new_v0(b"z").unwrap(), 1, 1),
            cid!([1; 64].as_ref()),
            cid!([[1].as_ref(), &[18; 63]].concat()),
        ];

        for cid in sample_cids.iter().chain(non_sample_cids.iter()) {
            blockstore.put_keyed(cid, &[10; 150]).await.unwrap();
        }

        for cid in &sample_cids {
            assert!(blockstore.has(cid).await.unwrap());
        }
        for cid in &non_sample_cids {
            assert!(!blockstore.has(cid).await.unwrap());
        }
    }
}
