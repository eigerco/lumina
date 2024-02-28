use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use celestia_tendermint::merkle::simple_hash_from_byte_vectors;
use celestia_tendermint_proto::Protobuf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::consts::data_availability_header::{
    MAX_EXTENDED_SQUARE_WIDTH, MIN_EXTENDED_SQUARE_WIDTH,
};
use crate::hash::Hash;
use crate::nmt::{NamespacedHash, NamespacedHashExt};
use crate::rsmt2d::AxisType;
use crate::{bail_validation, Error, ExtendedDataSquare, Result, ValidateBasic, ValidationError};

/// Header with commitments of the data availability.
///
/// It consists of the root hashes of the merkle trees created from each
/// row and column of the [`ExtendedDataSquare`]. Those are used to prove
/// the inclusion of the data in a block.
///
/// The hash of this header is a hash of all rows and columns and thus a
/// data commitment of the block.
///
/// # Example
///
/// ```no_run
/// # use celestia_types::{ExtendedHeader, Height, Share};
/// # use celestia_types::nmt::{Namespace, NamespaceProof};
/// # fn extended_header() -> ExtendedHeader {
/// #     unimplemented!();
/// # }
/// # fn shares_with_proof(_: Height, _: &Namespace) -> (Vec<Share>, NamespaceProof) {
/// #     unimplemented!();
/// # }
/// // fetch the block header and data for your namespace
/// let namespace = Namespace::new_v0(&[1, 2, 3, 4]).unwrap();
/// let eh = extended_header();
/// let (shares, proof) = shares_with_proof(eh.height(), &namespace);
///
/// // get the data commitment for a given row
/// let dah = eh.dah;
/// let root = dah.row_root(0).unwrap();
///
/// // verify a proof of the inclusion of the shares
/// assert!(proof.verify_complete_namespace(&root, &shares, *namespace).is_ok());
/// ```
///
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "RawDataAvailabilityHeader",
    into = "RawDataAvailabilityHeader"
)]
pub struct DataAvailabilityHeader {
    /// Merkle roots of the [`ExtendedDataSquare`] rows.
    pub row_roots: Vec<NamespacedHash>,
    /// Merkle roots of the [`ExtendedDataSquare`] columns.
    pub column_roots: Vec<NamespacedHash>,
}

impl DataAvailabilityHeader {
    /// Create a DataAvailabilityHeader by computing roots of a given [`ExtendedDataSquare`].
    pub fn from_eds(eds: &ExtendedDataSquare) -> Self {
        let square_len = eds.square_len();

        let mut dah = DataAvailabilityHeader {
            row_roots: Vec::with_capacity(square_len),
            column_roots: Vec::with_capacity(square_len),
        };

        for i in 0..square_len {
            let row_root = eds
                .row_nmt(i)
                .expect("EDS validated on construction")
                .root();
            dah.row_roots.push(row_root);

            let column_root = eds
                .column_nmt(i)
                .expect("EDS validated on construction")
                .root();
            dah.column_roots.push(column_root);
        }

        dah
    }

    /// Get the root from an axis at the given index.
    pub fn root(&self, axis: AxisType, index: usize) -> Option<NamespacedHash> {
        match axis {
            AxisType::Col => self.column_root(index),
            AxisType::Row => self.row_root(index),
        }
    }

    /// Get a root of the row with the given index.
    pub fn row_root(&self, row: usize) -> Option<NamespacedHash> {
        self.row_roots.get(row).cloned()
    }

    /// Get the a root of the column with the given index.
    pub fn column_root(&self, column: usize) -> Option<NamespacedHash> {
        self.column_roots.get(column).cloned()
    }

    /// Compute the combined hash of all rows and columns.
    ///
    /// This is the data commitment for the block.
    ///
    /// # Example
    ///
    /// ```
    /// # use celestia_types::ExtendedHeader;
    /// # fn get_extended_header() -> ExtendedHeader {
    /// #   let s = include_str!("../test_data/chain1/extended_header_block_1.json");
    /// #   serde_json::from_str(s).unwrap()
    /// # }
    /// let eh = get_extended_header();
    /// let dah = eh.dah;
    ///
    /// assert_eq!(dah.hash(), eh.header.data_hash);
    /// ```
    pub fn hash(&self) -> Hash {
        let all_roots: Vec<_> = self
            .row_roots
            .iter()
            .chain(self.column_roots.iter())
            .map(|root| root.to_array())
            .collect();

        Hash::Sha256(simple_hash_from_byte_vectors::<Sha256>(&all_roots))
    }

    /// Get the size of the [`ExtendedDataSquare`] for which this header was built.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn square_len(&self) -> usize {
        // `validate_basic` checks that rows num = cols num
        self.row_roots.len()
    }
}

impl Protobuf<RawDataAvailabilityHeader> for DataAvailabilityHeader {}

impl TryFrom<RawDataAvailabilityHeader> for DataAvailabilityHeader {
    type Error = Error;

    fn try_from(value: RawDataAvailabilityHeader) -> Result<Self, Self::Error> {
        Ok(DataAvailabilityHeader {
            row_roots: value
                .row_roots
                .iter()
                .map(|bytes| NamespacedHash::from_raw(bytes))
                .collect::<Result<Vec<_>>>()?,
            column_roots: value
                .column_roots
                .iter()
                .map(|bytes| NamespacedHash::from_raw(bytes))
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl From<DataAvailabilityHeader> for RawDataAvailabilityHeader {
    fn from(value: DataAvailabilityHeader) -> RawDataAvailabilityHeader {
        RawDataAvailabilityHeader {
            row_roots: value.row_roots.iter().map(|hash| hash.to_vec()).collect(),
            column_roots: value
                .column_roots
                .iter()
                .map(|hash| hash.to_vec())
                .collect(),
        }
    }
}

impl ValidateBasic for DataAvailabilityHeader {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.column_roots.len() != self.row_roots.len() {
            bail_validation!(
                "column_roots len ({}) != row_roots len ({})",
                self.column_roots.len(),
                self.row_roots.len(),
            )
        }

        if self.row_roots.len() < MIN_EXTENDED_SQUARE_WIDTH {
            bail_validation!(
                "row_roots len ({}) < minimum ({})",
                self.row_roots.len(),
                MIN_EXTENDED_SQUARE_WIDTH,
            )
        }

        if self.row_roots.len() > MAX_EXTENDED_SQUARE_WIDTH {
            bail_validation!(
                "row_roots len ({}) > maximum ({})",
                self.row_roots.len(),
                MAX_EXTENDED_SQUARE_WIDTH,
            )
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_dah() -> DataAvailabilityHeader {
        serde_json::from_str(r#"{
          "row_roots": [
            "//////////////////////////////////////7//////////////////////////////////////huZWOTTDmD36N1F75A9BshxNlRasCnNpQiWqIhdVHcU",
            "/////////////////////////////////////////////////////////////////////////////5iieeroHBMfF+sER3JpvROIeEJZjbY+TRE0ntADQLL3"
          ],
          "column_roots": [
            "//////////////////////////////////////7//////////////////////////////////////huZWOTTDmD36N1F75A9BshxNlRasCnNpQiWqIhdVHcU",
            "/////////////////////////////////////////////////////////////////////////////5iieeroHBMfF+sER3JpvROIeEJZjbY+TRE0ntADQLL3"
          ]
        }"#).unwrap()
    }

    #[test]
    fn validate_correct() {
        let dah = sample_dah();

        dah.validate_basic().unwrap();
    }

    #[test]
    fn validate_rows_and_cols_len_mismatch() {
        let mut dah = sample_dah();
        dah.row_roots.pop();

        dah.validate_basic().unwrap_err();
    }

    #[test]
    fn validate_too_little_square() {
        let mut dah = sample_dah();
        dah.row_roots = dah
            .row_roots
            .into_iter()
            .cycle()
            .take(MIN_EXTENDED_SQUARE_WIDTH)
            .collect();
        dah.column_roots = dah
            .column_roots
            .into_iter()
            .cycle()
            .take(MIN_EXTENDED_SQUARE_WIDTH)
            .collect();

        dah.validate_basic().unwrap();

        dah.row_roots.pop();
        dah.column_roots.pop();

        dah.validate_basic().unwrap_err();
    }

    #[test]
    fn validate_too_big_square() {
        let mut dah = sample_dah();
        dah.row_roots = dah
            .row_roots
            .into_iter()
            .cycle()
            .take(MAX_EXTENDED_SQUARE_WIDTH)
            .collect();
        dah.column_roots = dah
            .column_roots
            .into_iter()
            .cycle()
            .take(MAX_EXTENDED_SQUARE_WIDTH)
            .collect();

        dah.validate_basic().unwrap();

        dah.row_roots.push(dah.row_roots[0].clone());
        dah.column_roots.push(dah.column_roots[0].clone());

        dah.validate_basic().unwrap_err();
    }
}
