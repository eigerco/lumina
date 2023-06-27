use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tendermint::merkle::{simple_hash_from_byte_vectors, Hash};
use tendermint_proto::Protobuf;

use crate::consts::data_availability_header::{
    MAX_EXTENDED_SQUARE_WIDTH, MIN_EXTENDED_SQUARE_WIDTH,
};
use crate::{Error, Result, ValidateBasic, ValidationError, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    try_from = "RawDataAvailabilityHeader",
    into = "RawDataAvailabilityHeader"
)]
pub struct DataAvailabilityHeader {
    pub row_roots: Vec<Vec<u8>>,
    pub column_roots: Vec<Vec<u8>>,
    pub hash: Hash,
}

impl Protobuf<RawDataAvailabilityHeader> for DataAvailabilityHeader {}

impl TryFrom<RawDataAvailabilityHeader> for DataAvailabilityHeader {
    type Error = Error;

    fn try_from(value: RawDataAvailabilityHeader) -> Result<Self, Self::Error> {
        let all_roots: Vec<_> = value
            .row_roots
            .iter()
            .chain(value.column_roots.iter())
            .collect();
        let hash = simple_hash_from_byte_vectors::<Sha256>(&all_roots);

        Ok(DataAvailabilityHeader {
            row_roots: value.row_roots,
            column_roots: value.column_roots,
            hash,
        })
    }
}

impl From<DataAvailabilityHeader> for RawDataAvailabilityHeader {
    fn from(value: DataAvailabilityHeader) -> RawDataAvailabilityHeader {
        RawDataAvailabilityHeader {
            row_roots: value.row_roots,
            column_roots: value.column_roots,
        }
    }
}

impl ValidateBasic for DataAvailabilityHeader {
    fn validate_basic(&self) -> ValidationResult<()> {
        if self.column_roots.len() != self.row_roots.len() {
            return Err(ValidationError::NotASquare(
                self.column_roots.len(),
                self.row_roots.len(),
            ));
        }

        if self.row_roots.len() < MIN_EXTENDED_SQUARE_WIDTH {
            return Err(ValidationError::TooLittleSquare(self.row_roots.len()));
        }

        if self.row_roots.len() > MAX_EXTENDED_SQUARE_WIDTH {
            return Err(ValidationError::TooBigSquare(self.row_roots.len()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(matches!(
            dah.validate_basic(),
            Err(ValidationError::NotASquare(..))
        ));
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

        assert!(matches!(
            dah.validate_basic(),
            Err(ValidationError::TooLittleSquare(..))
        ));
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

        assert!(matches!(
            dah.validate_basic(),
            Err(ValidationError::TooBigSquare(..))
        ));
    }
}
