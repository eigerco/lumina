use std::result::Result as StdResult;

use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Deserializer, Serialize};

use crate::namespaced_data::{NamespacedData, NamespacedDataId};
use crate::nmt::{Namespace, NamespaceProof, NamespacedSha2Hasher, Nmt, NS_SIZE};
use crate::row::RowId;
use crate::{DataAvailabilityHeader, Error, Result};

/// Represents either column or row of the [`ExtendedDataSquare`].
///
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AxisType {
    /// A row of the data square.
    Row = 0,
    /// A column of the data square.
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

/// The data matrix in Celestia blocks extended with parity data.
///
/// It is created by a fixed size chunks of data, called [`Share`]s.
/// Each share is a cell of the [`ExtendedDataSquare`].
///
/// # Structure
///
/// The [`ExtendedDataSquare`] consists of four quadrants. The first
/// quadrant (upper-left) is the original data submitted to the network,
/// referred to as `OriginalDataSquare`. The other three quadrants are
/// the parity data encoded row-wise or column-wise using Reed-Solomon
/// `codec` specified in `EDS`.
///
/// The below diagram shows how the `EDS` is constructed. First, the 2nd
/// and 3rd quadrants are created by computing Reed-Solomon parity data
/// of the original data square, row-wise for 2nd and column-wise for
/// 3rd quadrant. Then, the 4th quadrant is computed either row-wise
/// from 3rd or column-wise from 2nd quadrant.
///
/// ```text
///  ---------------------------
/// |             |             |
/// |           --|->           |
/// |      1    --|->    2      |
/// |           --|->           |
/// |    | | |    |             |
///  -------------+-------------
/// |    v v v    |             |
/// |           --|->           |
/// |      3    --|->    4      |
/// |           --|->           |
/// |             |             |
///  ---------------------------
/// ```
///
/// # Data availability
///
/// The [`DataAvailabilityHeader`] is created by computing [`Nmt`] merkle
/// roots of each row and column of [`ExtendedDataSquare`].
/// By putting those together there are some key
/// properties those have in terms of data availability.
///
/// Thanks to the parity data, to make original data unrecoverable, a malicious
/// actor would need to hide more than a half of the data from each row and column.
/// If we take `k` as the width of the `OriginalDataSquare`, then the attacker
/// needs to hide more than `(k + 1)^2` shares from the [`ExtendedDataSquare`].
/// For the `EDS` with a width of 4, the attacker needs to hide more than 50% of
/// all the shares and that value approaches 25% as the square grows.
///
/// This allows for really efficient data sampling, as the sampling node can reach
/// very high confidence that whole data is available by taking only a few samples.
///
/// # Example
///
/// This example shows rebuilding the merkle trees for each row of the EDS and compares
/// them with the root hashes stored in data availability header.
///
/// ```no_run
/// use celestia_types::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
/// use celestia_types::Share;
/// use nmt_rs::NamespaceMerkleHasher;
/// # use celestia_types::{ExtendedDataSquare, ExtendedHeader};
/// # fn get_header(_: usize) -> ExtendedHeader {
/// #     unimplemented!()
/// # }
/// # fn get_eds(_: usize) -> ExtendedDataSquare {
/// #     unimplemented!()
/// # }
///
/// let block_height = 15;
/// let header = get_header(block_height);
/// let eds = get_eds(block_height);
/// let width = header.dah.square_len();
///
/// // for each row of the data square, build an nmt
/// for (y, row) in eds.data_square().chunks(width).enumerate() {
///     let mut nmt = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
///
///     for (x, leaf) in row.iter().enumerate() {
///         if x < width / 2 && y < width / 2 {
///             // the `OriginalDataSquare` part of the `EDS`
///             let share = Share::from_raw(leaf).unwrap();
///             let ns = share.namespace();
///             nmt.push_leaf(share.as_ref(), *ns).unwrap();
///         } else {
///             // the parity data computed using `eds.codec`
///             nmt.push_leaf(leaf, *Namespace::PARITY_SHARE).unwrap();
///         }
///     }
///
///     // check if the root corresponds to the one from the dah
///     let root = nmt.root();
///     assert_eq!(root, header.dah.row_root(y).unwrap());
/// }
/// ```
///
/// [`Nmt`]: crate::nmt::Nmt
/// [`Share`]: crate::share::Share
/// [`DataAvailabilityHeader`]: crate::DataAvailabilityHeader
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtendedDataSquare {
    /// The raw data of the EDS.
    #[serde(with = "celestia_tendermint_proto::serializers::bytes::vec_base64string")]
    data_square: Vec<Vec<u8>>,
    /// The codec used to encode parity shares.
    codec: String,
    /// pre-calculated square length
    #[serde(skip)]
    square_len: usize,
}

impl ExtendedDataSquare {
    /// Create a new EDS out of the provided shares. Returns error if number of shares isn't
    /// a square number
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

    /// The raw data of the EDS.
    pub fn data_square(&self) -> &[Vec<u8>] {
        &self.data_square
    }

    /// The codec used to encode parity shares.
    pub fn codec(&self) -> &str {
        self.codec.as_str()
    }

    pub fn share(&self, row_index: usize, column_index: usize) -> Result<&[u8]> {
        let index = row_index * self.square_len + column_index;

        self.data_square
            .get(index)
            .map(Vec::as_slice)
            .ok_or(Error::EdsIndexOutOfRange(index))
    }

    /// Return row with index
    pub fn row(&self, row_index: usize) -> Result<Vec<Vec<u8>>> {
        (0..self.square_len)
            .map(|column_index| self.share(row_index, column_index).map(ToOwned::to_owned))
            .collect()
    }

    /// Returns the [`Nmt`] of a row
    pub fn row_nmt(&self, row_index: usize) -> Result<Nmt> {
        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));

        for column_index in 0..self.square_len {
            let share = self.share(row_index, column_index)?;

            let ns = if column_index < self.square_len / 2 {
                Namespace::from_raw(&share[..NS_SIZE])?
            } else {
                Namespace::PARITY_SHARE
            };

            tree.push_leaf(share, *ns).map_err(Error::Nmt)?;
        }

        Ok(tree)
    }

    /// Return colum with index
    pub fn column(&self, column_index: usize) -> Result<Vec<Vec<u8>>> {
        (0..self.square_len)
            .map(|row_index| self.share(row_index, column_index).map(ToOwned::to_owned))
            .collect()
    }

    /// Returns the [`Nmt`] of a column
    pub fn column_nmt(&self, column_index: usize) -> Result<Nmt> {
        let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));

        for row_index in 0..self.square_len {
            let share = self.share(row_index, column_index)?;

            let ns = if row_index < self.square_len / 2 {
                Namespace::from_raw(&share[..NS_SIZE])?
            } else {
                Namespace::PARITY_SHARE
            };

            tree.push_leaf(share, *ns).map_err(Error::Nmt)?;
        }

        Ok(tree)
    }

    /// Return column or row with the provided index
    pub fn axis(&self, axis: AxisType, index: usize) -> Result<Vec<Vec<u8>>> {
        match axis {
            AxisType::Col => self.column(index),
            AxisType::Row => self.row(index),
        }
    }

    /// Returns the [`Nmt`] of a column or a row
    pub fn axis_nmt(&self, axis: AxisType, index: usize) -> Result<Nmt> {
        match axis {
            AxisType::Col => self.column_nmt(index),
            AxisType::Row => self.row_nmt(index),
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

        for i in 0u16..self.square_len as u16 {
            let row_root = dah.row_root(i.into()).unwrap();
            if !row_root.contains::<NamespacedSha2Hasher>(*namespace) {
                continue;
            }

            let mut shares = Vec::with_capacity(self.square_len);
            let mut tree = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
            for (col, s) in self.row(i.into())?.iter().enumerate() {
                let ns = if col < self.square_len / 2 {
                    Namespace::from_raw(&s[..NS_SIZE])?
                } else {
                    Namespace::PARITY_SHARE
                };

                tree.push_leaf(s, *ns).map_err(Error::Nmt)?;
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

#[derive(Deserialize)]
struct RawExtendedDataSquare {
    #[serde(with = "celestia_tendermint_proto::serializers::bytes::vec_base64string")]
    pub data_square: Vec<Vec<u8>>,
    pub codec: String,
}

impl<'de> Deserialize<'de> for ExtendedDataSquare {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let eds = RawExtendedDataSquare::deserialize(deserializer)?;
        let share_number = eds.data_square.len();
        ExtendedDataSquare::new(eds.data_square, eds.codec).map_err(|_| {
            <D::Error as serde::de::Error>::invalid_length(
                share_number,
                &"number of shares must be a perfect square",
            )
        })
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

    #[test]
    fn get_namespaced_data() {
        let eds_json = include_str!("../test_data/shwap_samples/eds.json");
        let eds: ExtendedDataSquare = serde_json::from_str(eds_json).unwrap();
        let dah_json = include_str!("../test_data/shwap_samples/dah.json");
        let dah: DataAvailabilityHeader = serde_json::from_str(dah_json).unwrap();

        let height = 45577;

        let rows = eds
            .get_namespaced_data(Namespace::new_v0(&[1, 170]).unwrap(), &dah, height)
            .unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        row.verify(&dah).unwrap();
        assert_eq!(row.shares.len(), 2);

        let rows = eds
            .get_namespaced_data(Namespace::new_v0(&[1, 187]).unwrap(), &dah, height)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].shares.len(), 1);
        assert_eq!(rows[1].shares.len(), 4);
        for row in rows {
            row.verify(&dah).unwrap();
        }
    }
}
