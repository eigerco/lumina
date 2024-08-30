use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use celestia_tendermint::merkle::simple_hash_from_byte_vectors;
use celestia_tendermint_proto::v0_34::types::RowProof as RawRowProof;
use celestia_tendermint_proto::Protobuf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::consts::data_availability_header::{
    MAX_EXTENDED_SQUARE_WIDTH, MIN_EXTENDED_SQUARE_WIDTH,
};
use crate::hash::Hash;
use crate::nmt::{NamespacedHash, NamespacedHashExt};
use crate::rsmt2d::AxisType;
use crate::{
    bail_validation, bail_verification, Error, ExtendedDataSquare, MerkleProof, Result,
    ValidateBasic, ValidationError,
};

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
    row_roots: Vec<NamespacedHash>,
    /// Merkle roots of the [`ExtendedDataSquare`] columns.
    column_roots: Vec<NamespacedHash>,
}

impl DataAvailabilityHeader {
    /// Create new [`DataAvailabilityHeader`].
    pub fn new(row_roots: Vec<NamespacedHash>, column_roots: Vec<NamespacedHash>) -> Result<Self> {
        let dah = DataAvailabilityHeader::new_unchecked(row_roots, column_roots);
        dah.validate_basic()?;
        Ok(dah)
    }

    /// Create new non-validated [`DataAvailabilityHeader`].
    ///
    /// [`DataAvailabilityHeader::validate_basic`] can be used to check valitidy later on.
    pub fn new_unchecked(
        row_roots: Vec<NamespacedHash>,
        column_roots: Vec<NamespacedHash>,
    ) -> Self {
        DataAvailabilityHeader {
            row_roots,
            column_roots,
        }
    }

    /// Create a DataAvailabilityHeader by computing roots of a given [`ExtendedDataSquare`].
    pub fn from_eds(eds: &ExtendedDataSquare) -> Self {
        let square_width = eds.square_width();

        let mut dah = DataAvailabilityHeader {
            row_roots: Vec::with_capacity(square_width.into()),
            column_roots: Vec::with_capacity(square_width.into()),
        };

        for i in 0..square_width {
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
    pub fn root(&self, axis: AxisType, index: u16) -> Option<NamespacedHash> {
        match axis {
            AxisType::Col => self.column_root(index),
            AxisType::Row => self.row_root(index),
        }
    }

    /// Merkle roots of the [`ExtendedDataSquare`] rows.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn row_roots(&self) -> &[NamespacedHash] {
        &self.row_roots
    }

    /// Merkle roots of the [`ExtendedDataSquare`] columns.
    ///
    /// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
    pub fn column_roots(&self) -> &[NamespacedHash] {
        &self.column_roots
    }

    /// Get a root of the row with the given index.
    pub fn row_root(&self, row: u16) -> Option<NamespacedHash> {
        let row = usize::from(row);
        self.row_roots.get(row).cloned()
    }

    /// Get the a root of the column with the given index.
    pub fn column_root(&self, column: u16) -> Option<NamespacedHash> {
        let column = usize::from(column);
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
    pub fn square_width(&self) -> u16 {
        // `validate_basic` checks that rows num = cols num
        self.row_roots
            .len()
            .try_into()
            // On validated DAH this never happens
            .expect("len is bigger than u16::MAX")
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawRowProof", into = "RawRowProof")]
pub struct RowProof {
    row_roots: Vec<NamespacedHash>,
    proofs: Vec<MerkleProof>,
    start_row: usize,
    end_row: usize,
}

impl RowProof {
    pub fn verify(&self, root: Hash) -> Result<()> {
        if self.row_roots.len() != self.proofs.len() {
            bail_verification!("invalid row proof: row_roots.len() != proofs.len()");
        }
        let Hash::Sha256(root) = root else {
            bail_verification!("empty hash");
        };

        for (row_root, proof) in self.row_roots.iter().zip(self.proofs.iter()) {
            proof.verify(row_root.to_array(), root)?;
        }

        Ok(())
    }
}

impl Protobuf<RawRowProof> for RowProof {}

impl TryFrom<RawRowProof> for RowProof {
    type Error = Error;

    fn try_from(value: RawRowProof) -> Result<Self> {
        Ok(Self {
            row_roots: value
                .row_roots
                .into_iter()
                .map(|hash| NamespacedHash::from_raw(&hash))
                .collect::<Result<_>>()?,
            proofs: value
                .proofs
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_>>()?,
            start_row: value.start_row as usize,
            end_row: value.end_row as usize,
        })
    }
}

impl From<RowProof> for RawRowProof {
    fn from(value: RowProof) -> Self {
        Self {
            row_roots: value
                .row_roots
                .into_iter()
                .map(|hash| hash.to_vec())
                .collect(),
            proofs: value.proofs.into_iter().map(Into::into).collect(),
            start_row: value.start_row as u32,
            end_row: value.end_row as u32,
            root: vec![],
        }
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

    #[test]
    fn row_proof_verify_correct() {
        let raw_row_proof = r#"
          {
            "end_row": 1,
            "proofs": [
              {
                "aunts": [
                  "Ch+9PsBdsN5YUt8nvAmjdOAIcVdfmPAEUNmCA8KBe5A=",
                  "ojjC9H5JG/7OOrt5BzBXs/3w+n1LUI/0YR0d+RSfleU=",
                  "d6bMQbLTBfZGvqXOW9MPqRM+fTB2/wLJx6CkLc8glCI="
                ],
                "index": 0,
                "leaf_hash": "nOpM3A3d0JYOmNaaI5BFAeKPGwQ90TqmM/kx+sHr79s=",
                "total": 8
              },
              {
                "aunts": [
                  "nOpM3A3d0JYOmNaaI5BFAeKPGwQ90TqmM/kx+sHr79s=",
                  "ojjC9H5JG/7OOrt5BzBXs/3w+n1LUI/0YR0d+RSfleU=",
                  "d6bMQbLTBfZGvqXOW9MPqRM+fTB2/wLJx6CkLc8glCI="
                ],
                "index": 1,
                "leaf_hash": "Ch+9PsBdsN5YUt8nvAmjdOAIcVdfmPAEUNmCA8KBe5A=",
                "total": 8
              }
            ],
            "row_roots": [
              "000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000D8CBB533A24261C4C0A3D37F1CBFB6F4C5EA031472EBA390D482637933874AA0A2B9735E67629993852D",
              "00000000000000000000000000000000000000D8CBB533A24261C4C0A300000000000000000000000000000000000000D8CBB533A24261C4C0A37E409334CCB1125C793EC040741137634C148F089ACB06BFFF4C1C4CA2CBBA8E"
            ],
            "start_row": 0
          }
        "#;
        let raw_dah = r#"
          {
            "row_roots": [
              "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAo9N/HL+29MXqAxRy66OQ1IJjeTOHSqCiuXNeZ2KZk4Ut",
              "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAAAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAo35AkzTMsRJceT7AQHQRN2NMFI8ImssGv/9MHEyiy7qO",
              "/////////////////////////////////////////////////////////////////////////////7mTwL+NxdxcYBd89/wRzW2k9vRkQehZiXsuqZXHy89X",
              "/////////////////////////////////////////////////////////////////////////////2X/FT2ugeYdWmvnEisSgW+9Ih8paNvrji2NYPb8ujaK"
            ],
            "column_roots": [
              "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAo/xEv//wkWzNtkcAZiZmSGU1Te6ERwUxTtTfHzoS4bv+",
              "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAAAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAo9FOCNvCjA42xYCwHrlo48iPEXLaKt+d+JdErCIrQIi6",
              "/////////////////////////////////////////////////////////////////////////////y2UErq/83uv433HekCWokxqcY4g+nMQn3tZn2Tr6v74",
              "/////////////////////////////////////////////////////////////////////////////z6fKmbJTvfLYFlNuDWHn87vJb6V7n44MlCkxv1dyfT2"
            ]
          }
        "#;

        let row_proof: RowProof = serde_json::from_str(raw_row_proof).unwrap();
        let dah: DataAvailabilityHeader = serde_json::from_str(raw_dah).unwrap();

        row_proof.verify(dah.hash()).unwrap();
    }
}
