use std::result::Result as StdResult;

use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};

use crate::namespaced_data::{NamespacedData, NamespacedDataId};
use crate::nmt::{Namespace, NamespacedSha2Hasher, Nmt, NS_SIZE};
use crate::row::RowId;
use crate::{DataAvailabilityHeader, Error, Result};

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
    pub data_square: Vec<Vec<u8>>,
    pub codec: String,
    #[serde(skip)]
    pub square_len: usize,
}

impl ExtendedDataSquare {
    /// Create a new EDS out of the provided shares. Returns error if share number doesn't
    /// create a square
    pub fn new(shares: Vec<Vec<u8>>, codec: String) -> Result<Self> {
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
    pub fn row(&self, index: usize) -> Result<Vec<Vec<u8>>> {
        Ok(self
            .data_square
            .get(index * self.square_len..(index + 1) * self.square_len)
            .ok_or(Error::EdsIndexOutOfRange(index))?
            .to_vec())
    }

    /// Return colum with index
    pub fn column(&self, mut index: usize) -> Result<Vec<Vec<u8>>> {
        let mut r = Vec::with_capacity(self.square_len);
        while index < self.data_square.len() {
            r.push(
                self.data_square
                    .get(index)
                    .ok_or(Error::EdsIndexOutOfRange(index))?
                    .to_vec(),
            );
            index += self.square_len;
        }
        Ok(r)
    }

    /// Return column or row with the provided index
    pub fn axis(&self, axis: AxisType, index: usize) -> Result<Vec<Vec<u8>>> {
        match axis {
            AxisType::Col => self.column(index),
            AxisType::Row => self.row(index),
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
                let ns = Namespace::from_raw(&s[..NS_SIZE])?;
                tree.push_leaf(&s, *ns).map_err(Error::Nmt)?;
                if ns == namespace {
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
}
