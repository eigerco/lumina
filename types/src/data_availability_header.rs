use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tendermint::merkle::simple_hash_from_byte_vectors;
use tendermint_proto::Protobuf;

use crate::consts::data_availability_header::{
    MAX_EXTENDED_SQUARE_WIDTH, MIN_EXTENDED_SQUARE_WIDTH,
};
use crate::hash::Hash;
use crate::nmt::{NamespacedHash, NamespacedHashExt};
use crate::{bail_validation, Error, Result, ValidateBasic, ValidationError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "RawDataAvailabilityHeader",
    into = "RawDataAvailabilityHeader"
)]
pub struct DataAvailabilityHeader {
    pub row_roots: Vec<NamespacedHash>,
    pub column_roots: Vec<NamespacedHash>,
}

impl DataAvailabilityHeader {
    pub fn row_root(&self, row: usize) -> Option<NamespacedHash> {
        self.row_roots.get(row).cloned()
    }

    pub fn column_root(&self, column: usize) -> Option<NamespacedHash> {
        self.column_roots.get(column).cloned()
    }

    pub fn hash(&self) -> Hash {
        let all_roots: Vec<_> = self
            .row_roots
            .iter()
            .chain(self.column_roots.iter())
            .map(|root| root.to_array())
            .collect();

        Hash::Sha256(simple_hash_from_byte_vectors::<Sha256>(&all_roots))
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
