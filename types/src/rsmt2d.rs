use std::result::Result as StdResult;

use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};

use crate::namespaced_data::{NamespacedData, NamespacedDataId};
use crate::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
use crate::row::RowId;
use crate::{DataAvailabilityHeader, Share};
use crate::{Error, Result};

/// Represents either Column or Row of the Data Square.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AxisType {
    Row = 0,
    Col,
}

impl TryFrom<u8> for AxisType {
    type Error = Error;

    fn try_from(value: u8) -> StdResult<Self, Self::Error> {
        match value {
            0 => Ok(AxisType::Row),
            1 => Ok(AxisType::Col),
            n => Err(Error::InvalidAxis(n.into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedDataSquare {
    pub data_square: Vec<Share>,
    pub codec: String,
    #[serde(skip)]
    pub square_len: usize,
}

impl ExtendedDataSquare {
    /// Create a new EDS out of the provided shares. Returns error if share number doesn't
    /// create a square
    pub fn new(shares: Vec<Share>, codec: String) -> Result<Self> {
        let square_len = f64::sqrt(shares.len() as f64) as usize;
        if square_len * square_len != shares.len() {
            return Err(Error::EdsInvalidDimentions);
        }

        Ok(Self {
            data_square: shares,
            codec,
            square_len,
        })
    }

    /// Return row with index
    pub fn row(&self, index: usize) -> Result<&[Share]> {
        self.data_square
            .get(index * self.square_len..(index + 1) * self.square_len)
            .ok_or(Error::EdsIndexOutOfRange(index))
    }

    /// Return colum with index
    pub fn column(&self, mut index: usize) -> Result<Vec<&Share>> {
        let mut r = Vec::with_capacity(self.square_len);
        while index < self.data_square.len() {
            r.push(
                self.data_square
                    .get(index)
                    .ok_or(Error::EdsIndexOutOfRange(index))?,
            );
            index += self.square_len;
        }
        Ok(r)
    }

    /// Return column or row with the provided index
    pub fn axis(&self, axis: AxisType, index: usize) -> Result<Vec<&Share>> {
        match axis {
            AxisType::Col => self.column(index),
            AxisType::Row => Ok(self.row(index)?.iter().collect()),
        }
    }

    /// Get EDS square length
    pub fn square_len(&self) -> usize {
        self.square_len
    }

    /// Return all the shares that belong to the provided namespace in the EDS.
    /// Results are returned as a list of rows of shares with the inclusion proof
    pub fn get_namespaced_data(
        &self,
        namespace: Namespace,
        dah: &DataAvailabilityHeader,
        height: u64,
    ) -> Result<Vec<NamespacedData>> {
        let mut data = Vec::new();

        for i in 0..self.square_len {
            let row_root = dah.row_root(i).unwrap();
            if !row_root.contains::<NamespacedSha2Hasher>(*namespace) {
                continue;
            }

            let mut shares = Vec::with_capacity(self.square_len);

            let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
            for s in self.row(i)? {
                tree.push_leaf(s.data(), *s.namespace())
                    .map_err(Error::Nmt)?;
                if s.namespace() == namespace {
                    shares.push(s.clone());
                }
            }
            let row = RowId::new(i, height)?;

            let proof = tree.get_namespace_proof(*namespace);
            let namespaced_data_id = NamespacedDataId { row, namespace };

            data.push(NamespacedData {
                namespaced_data_id,
                proof: proof.into(),
                shares,
            })
        }

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmt_rs::NamespacedHash;

    #[test]
    fn axis_type_serialization() {
        assert_eq!(AxisType::Row as u8, 0);
        assert_eq!(AxisType::Col as u8, 1);
    }

    #[test]
    fn axis_type_deserialization() {
        assert_eq!(AxisType::try_from(0).unwrap(), AxisType::Row);
        assert_eq!(AxisType::try_from(1).unwrap(), AxisType::Col);

        let axis_type_err = AxisType::try_from(2).unwrap_err();
        assert!(matches!(axis_type_err, Error::InvalidAxis(2)));
        let axis_type_err = AxisType::try_from(99).unwrap_err();
        assert!(matches!(axis_type_err, Error::InvalidAxis(99)));
    }

    #[test]
    fn get_empty_namespaced_data() {
        // we're building EDS with following namespace|data
        //
        // 1|1 1|2 1|3 2|1
        // 2|2 2|3 2|4 2|5
        // 2|6 3|1 3|2 3|3
        // 3|4  X   X   X
        //
        // where X is a padding share

        #[rustfmt::skip]
        let eds_prototype = vec![
            (1,1), (1,2), (1,3), (2,1),
            (2,2), (2,3), (2,4), (2,5),
            (2,6), (3,1), (3,2), (3,3),
            (3,4)
        ];
        let mut shares = eds_prototype
            .into_iter()
            .map(|(ns, data)| {
                let ns = Namespace::new_v0(&[ns]).unwrap();
                Share::new_v0(ns, None, &[data]).unwrap()
            })
            .collect::<Vec<_>>();
        shares.resize(
            16,
            Share::new_v0(Namespace::TAIL_PADDING, None, &[]).unwrap(),
        );

        let eds = ExtendedDataSquare::new(shares.clone(), "".to_string()).unwrap();

        let ns = [
            *Namespace::new_v0(&[0]).unwrap(),
            *Namespace::new_v0(&[1]).unwrap(),
            *Namespace::new_v0(&[2]).unwrap(),
            *Namespace::new_v0(&[3]).unwrap(),
        ];

        // we're relying on the fact that getting namespaced data only needs min and max
        // namespace data from row roots to be correct
        let row_roots = vec![
            NamespacedHash::new(ns[1], ns[2], [0; 32]),
            NamespacedHash::new(ns[2], ns[2], [0; 32]),
            NamespacedHash::new(ns[2], ns[3], [0; 32]),
            NamespacedHash::new(ns[3], *Namespace::TAIL_PADDING, [0; 32]),
        ];
        let dah = DataAvailabilityHeader {
            column_roots: row_roots.clone(),
            row_roots,
        };

        let namespaced_data1 = eds.get_namespaced_data(ns[1].into(), &dah, 1).unwrap();
        assert_eq!(namespaced_data1.len(), 1);
        assert_eq!(namespaced_data1[0].shares.len(), 3);
        assert_eq!(namespaced_data1[0].shares, shares[0..=2]);

        let namespaced_data2 = eds.get_namespaced_data(ns[2].into(), &dah, 1).unwrap();
        assert_eq!(namespaced_data2.len(), 3);
        assert_eq!(namespaced_data2[0].shares, shares[3..=3]);
        assert_eq!(namespaced_data2[1].shares, shares[4..=7]);
        assert_eq!(namespaced_data2[2].shares, shares[8..=8]);

        let namespaced_data3 = eds.get_namespaced_data(ns[3].into(), &dah, 1).unwrap();
        assert_eq!(namespaced_data3.len(), 2);
        assert_eq!(namespaced_data3[0].shares, shares[9..=11]);
        assert_eq!(namespaced_data3[1].shares, shares[12..=12]);
    }
}
