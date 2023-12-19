use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};

use crate::axis::AxisType;
use crate::namespaced_data::{NamespacedData, NamespacedDataId};
use crate::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
use crate::{DataAvailabilityHeader, Share};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedDataSquare {
    #[serde(with = "tendermint_proto::serializers::bytes::vec_base64string")]
    pub data_square: Vec<Vec<u8>>,
    pub codec: String,
}

impl ExtendedDataSquare {
    pub fn row(&self, index: usize, square_len: usize) -> Vec<Share> {
        self.data_square[index * square_len..(index + 1) * square_len]
            .iter()
            .map(|s| Share::from_raw(s))
            .collect::<Result<_, _>>()
            .unwrap()
    }

    pub fn column(&self, mut index: usize, square_len: usize) -> Vec<Share> {
        let mut r = Vec::with_capacity(square_len);
        while index < self.data_square.len() {
            r.push(Share::from_raw(&self.data_square[index]).unwrap());
            index += square_len;
        }
        r
    }

    pub fn axis(&self, axis: AxisType, index: usize, square_len: usize) -> Vec<Share> {
        match axis {
            AxisType::Col => self.column(index, square_len),
            AxisType::Row => self.row(index, square_len),
        }
    }

    pub fn get_namespaced_data(
        &self,
        namespace: Namespace,
        dah: &DataAvailabilityHeader,
        height: u64,
    ) -> Result<Vec<NamespacedData>> {
        let square_len = dah.square_len();

        let mut data = Vec::new();

        for i in 0..square_len {
            let row_root = dah.row_root(i).unwrap();
            if !row_root.contains::<NamespacedSha2Hasher>(*namespace) {
                continue;
            }

            let mut shares = Vec::with_capacity(square_len);

            let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
            for s in &self.data_square {
                let s = Share::from_raw(s).unwrap(); // TODO: swap entire tree to shares?
                tree.push_leaf(s.data(), *s.namespace())
                    .map_err(Error::Nmt)?;
                if s.namespace() == namespace {
                    shares.push(s);
                }
            }

            let proof = tree.get_namespace_proof(*namespace);
            let namespaced_data_id = NamespacedDataId {
                row_index: i.try_into().unwrap(),
                namespace,
                hash: dah.hash().as_ref().try_into().unwrap(),
                block_height: height,
            };

            data.push(NamespacedData {
                namespaced_data_id,
                proof: proof.into(),
                shares,
            })
        }

        Ok(data)
    }
}
