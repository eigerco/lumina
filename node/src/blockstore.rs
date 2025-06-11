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

impl SampleBlockstore<blockstore::InMemoryBlockstore<MAX_MH_SIZE>> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            blockstore: blockstore::InMemoryBlockstore::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SampleBlockstore<blockstore::RedbBlockstore> {
    pub fn new(db: std::sync::Arc<redb::Database>) -> Self {
        Self {
            blockstore: blockstore::RedbBlockstore::new(db),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl SampleBlockstore<blockstore::IndexedDbBlockstore> {
    pub async fn new(name: &str) -> Result<Self> {
        Self {
            blockstore: blockstore::IndexedDbBlockstore::new(name),
        }
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
    use cid::CidGeneric;
    use lumina_utils::test_utils::async_test;

    use crate::p2p::shwap::convert_cid;

    use super::InMemoryBlockstore;

    #[async_test]
    async fn should_only_store_samples() {
        let blockstore = InMemoryBlockstore::new();

        let sample_cids = [
            convert_cid(&SampleId::new(1, 2, 3).unwrap().into()).unwrap(),
            convert_cid(&SampleId::new(1111, 232, 33).unwrap().into()).unwrap(),
            convert_cid(&SampleId::new(123, 1, 888888888).unwrap().into()).unwrap(),
        ];

        let non_sample_cids = [
            convert_cid(&RowId::new(1, 1737).unwrap().into()).unwrap(),
            convert_cid(&RowId::new(8812, 193139).unwrap().into()).unwrap(),
            convert_cid(
                &RowNamespaceDataId::new(Namespace::new_v0(b"foo").unwrap(), 15, 12310)
                    .unwrap()
                    .into(),
            )
            .unwrap(),
            convert_cid(
                &RowNamespaceDataId::new(Namespace::new_v0(b"bar").unwrap(), 1, 1)
                    .unwrap()
                    .into(),
            )
            .unwrap(),
            CidGeneric::try_from([1; 64].as_ref()).unwrap(),
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
