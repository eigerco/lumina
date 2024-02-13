use std::sync::Arc;

use async_trait::async_trait;
use beetswap::multihasher::{Multihasher, MultihasherError};
use blockstore::block::CidError;
use celestia_tendermint_proto::Protobuf;
use celestia_types::namespaced_data::{
    NamespacedData, NamespacedDataId, NAMESPACED_DATA_ID_MULTIHASH_CODE,
};
use celestia_types::nmt::Namespace;
use celestia_types::row::{Row, RowId, ROW_ID_MULTIHASH_CODE};
use celestia_types::sample::{Sample, SampleId, SAMPLE_ID_MULTIHASH_CODE};
use cid::{Cid, CidGeneric};
use libp2p::multihash::Multihash;

use crate::p2p::{P2pError, Result, MAX_MH_SIZE};
use crate::store::Store;

/// Multihasher for Shwap types.
pub(super) struct ShwapMultihasher<S>
where
    S: Store + 'static,
{
    header_store: Arc<S>,
}

impl<S> ShwapMultihasher<S>
where
    S: Store + 'static,
{
    pub(super) fn new(header_store: Arc<S>) -> Self {
        ShwapMultihasher { header_store }
    }
}

#[async_trait]
impl<S> Multihasher<MAX_MH_SIZE> for ShwapMultihasher<S>
where
    S: Store + 'static,
{
    async fn hash(
        &self,
        multihash_code: u64,
        input: &[u8],
    ) -> Result<Multihash<MAX_MH_SIZE>, MultihasherError> {
        match multihash_code {
            NAMESPACED_DATA_ID_MULTIHASH_CODE => {
                let ns_data =
                    NamespacedData::decode(input).map_err(MultihasherError::custom_fatal)?;

                let hash = convert_cid(&ns_data.namespaced_data_id.into())
                    .map_err(MultihasherError::custom_fatal)?
                    .hash()
                    .to_owned();

                let header = self
                    .header_store
                    .get_by_height(ns_data.namespaced_data_id.row.block_height)
                    .await
                    .map_err(MultihasherError::custom_fatal)?;

                ns_data
                    .verify(&header.dah)
                    .map_err(MultihasherError::custom_fatal)?;

                Ok(hash)
            }
            ROW_ID_MULTIHASH_CODE => {
                let row = Row::decode(input).map_err(MultihasherError::custom_fatal)?;

                let hash = convert_cid(&row.row_id.into())
                    .map_err(MultihasherError::custom_fatal)?
                    .hash()
                    .to_owned();

                let header = self
                    .header_store
                    .get_by_height(row.row_id.block_height)
                    .await
                    .map_err(MultihasherError::custom_fatal)?;

                row.verify(&header.dah)
                    .map_err(MultihasherError::custom_fatal)?;

                Ok(hash)
            }
            SAMPLE_ID_MULTIHASH_CODE => {
                let sample = Sample::decode(input).map_err(MultihasherError::custom_fatal)?;

                let hash = convert_cid(&sample.sample_id.into())
                    .map_err(MultihasherError::custom_fatal)?
                    .hash()
                    .to_owned();

                let header = self
                    .header_store
                    .get_by_height(sample.sample_id.row.block_height)
                    .await
                    .map_err(MultihasherError::custom_fatal)?;

                sample
                    .verify(&header.dah)
                    .map_err(MultihasherError::custom_fatal)?;

                Ok(hash)
            }
            _ => Err(MultihasherError::UnknownMultihashCode),
        }
    }
}

pub(super) fn row_cid(row_index: u16, block_height: u64) -> Result<Cid> {
    let row_id = RowId::new(row_index, block_height).map_err(P2pError::Cid)?;
    convert_cid(&row_id.into())
}

pub(super) fn sample_cid(index: usize, square_len: usize, block_height: u64) -> Result<Cid> {
    let sample_id = SampleId::new(index, square_len, block_height).map_err(P2pError::Cid)?;
    convert_cid(&sample_id.into())
}

pub(super) fn namespaced_data_cid(
    namespace: Namespace,
    row_index: u16,
    block_height: u64,
) -> Result<Cid> {
    let data_id =
        NamespacedDataId::new(namespace, row_index, block_height).map_err(P2pError::Cid)?;
    convert_cid(&data_id.into())
}

pub(crate) fn convert_cid<const S: usize>(cid: &CidGeneric<S>) -> Result<Cid> {
    beetswap::utils::convert_cid(cid).ok_or(P2pError::Cid(celestia_types::Error::CidError(
        CidError::InvalidMultihashLength(64),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use celestia_types::consts::appconsts::SHARE_SIZE;
    use celestia_types::nmt::NS_SIZE;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use celestia_types::{DataAvailabilityHeader, ExtendedDataSquare};

    use celestia_types::nmt::{NamespaceMerkleHasher, NamespacedSha2Hasher, Nmt};
    use rand::RngCore;

    /*
    #[test]
    fn digest() {
        let hash = ShwapMultihasher
            .digest(
                0x7821,
                &[
                    10, 39, 6, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 26, 0,
                ],
            )
            .unwrap();

        let cid = "bagqpaanb6aasobqaaaaaaaaaaacqaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaq"
            .parse::<Cid>()
            .unwrap();
        let expected_hash = cid.hash();

        assert_eq!(hash, *expected_hash);

        }
    */

    fn random_namespaced_data(ns: Namespace) -> Vec<u8> {
        let mut buf = vec![0u8; SHARE_SIZE];

        buf[..NS_SIZE].copy_from_slice(ns.as_bytes());
        rand::thread_rng().fill_bytes(&mut buf[NS_SIZE..]);

        buf
    }

    fn generate_fake_eds() -> ExtendedDataSquare {
        let shares = vec![
            random_namespaced_data(Namespace::const_v0(rand::random())),
            random_namespaced_data(Namespace::PARITY_SHARE),
            random_namespaced_data(Namespace::PARITY_SHARE),
            random_namespaced_data(Namespace::PARITY_SHARE),
        ];

        ExtendedDataSquare::new(shares, "fake".to_string()).unwrap()
    }

    fn dah_of_eds(eds: &ExtendedDataSquare) -> DataAvailabilityHeader {
        let mut dah = DataAvailabilityHeader {
            row_roots: Vec::new(),
            column_roots: Vec::new(),
        };

        for i in 0..eds.square_len() {
            let row_root = eds.row_nmt(i).unwrap().root();
            dah.row_roots.push(row_root);

            let column_root = eds.column_nmt(i).unwrap().root();
            dah.column_roots.push(column_root);
        }

        dah
    }

    #[test]
    fn blabla() {
        let eds = generate_fake_eds();
        let dah = dah_of_eds(&eds);

        let mut gen = ExtendedHeaderGenerator::new();
        let header = gen.next_with_dah(dah);
    }
}
