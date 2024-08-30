use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use celestia_tendermint::merkle::simple_hash_from_byte_vectors;
use celestia_tendermint_proto::v0_34::crypto::Proof as MerkleProof;
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

// TODO: it should follow our regular try from / into RawRowProof pattern
// utilizing proto/vendor/tendermint/types/types.proto. However we cannot
// easily generate it, as it's in .tendermint.types package which we override
// with celestia_tendermint_proto. so the correct solution here would be to
// update celestia_tendermint_proto proto definitions to a recent celestia-core
// version. Note, this wouldn't free us from merkle proof verification logic,
// they are not present in tendermint-rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RowProof {
    #[serde(with = "hash_vec_hexstring")]
    row_roots: Vec<NamespacedHash>,
    proofs: Vec<MerkleProof>,
    #[serde(default, with = "hash_base64string")]
    root: Option<Hash>,
    start_row: usize,
    end_row: usize,
}

impl RowProof {
    fn verify(&self, root: Hash) -> Result<()> {
        assert_eq!(self.row_roots.len(), self.proofs.len());

        for (row_root, proof) in self.row_roots.iter().zip(self.proofs.iter()) {
            todo!()
            // verify_merkle_proof(proof, &root, row_root);
        }

        Ok(())
    }
}

impl Protobuf<RawRowProof> for RowProof {}

impl TryFrom<RawRowProof> for RowProof {
    type Error = Error;

    fn try_from(value: RawRowProof) -> Result<Self> {
        todo!()
    }
}

impl From<RowProof> for RawRowProof {
    fn from(value: RowProof) -> Self {
        todo!()
    }
}

fn verify_merkle_proof(proof: &MerkleProof, root: &Hash, leaf: &Hash) {}

mod hash_base64string {
    use base64::prelude::*;
    use serde::{de, Deserialize, Deserializer, Serializer};

    use super::Hash;

    pub fn serialize<S>(value: &Option<Hash>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(hash) = value {
            serializer.serialize_some(&BASE64_STANDARD.encode(hash))
        } else {
            serializer.serialize_none()
        }
    }

    /// Deserialize base64string into `Hash`
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Hash>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let Some(hash) = Option::<&str>::deserialize(deserializer)? else {
            return Ok(None);
        };
        BASE64_STANDARD
            .decode(hash)
            .map_err(de::Error::custom)?
            .try_into()
            .map(Some)
            .map_err(de::Error::custom)
    }
}

mod hash_vec_hexstring {
    use serde::{de, Deserialize, Deserializer, Serializer};

    use super::{NamespacedHash, NamespacedHashExt};

    /// Serialize from `Vec<NamespacedHash>` into `Vec<hexstring>`
    pub fn serialize<S>(value: &[NamespacedHash], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hashes: Vec<_> = value
            .iter()
            .map(|hash| hex::encode_upper(hash.to_array()))
            .collect();
        serializer.serialize_some(&hashes)
    }

    /// Deserialize vec_base64string into `NamespacedHash`
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<NamespacedHash>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<Vec<&str>>::deserialize(deserializer)?
            .unwrap_or_default()
            .into_iter()
            .map(|raw_hash| {
                hex::decode(raw_hash)
                    .map_err(de::Error::custom)
                    .and_then(|hash| NamespacedHash::from_raw(&hash).map_err(de::Error::custom))
            })
            .collect()
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
//   "Proof": {
//     "data": [
//       "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMBAAAEAAK7jTduoBTIVHIsXZYBTeXT+ROAP0ErS1wBn3qRFHoNClY8r4gEOLhvoPDfYX5dN+qGDHdIFPG4F1aF+niSmbfQRSkw2QdjqKwDKhYUKvu10oUo5r/k0SyYJx5KImSJ0d2sBH/ajcpk
// +DWBD0tXJTmsfiATmM8BqaxRRa5biE1T9yV1WKndAyJUC00P8e2/MG+7P1t7a9tMjG+Oxgxx1EzxJ47FDiRwnYNz2/JBqzIC33fKoiWZSeL+NFLn0Dfx+Ev1GYaKpstd1x1tgJnkEceTFVC6r7qhqRbTFJjAjgYJAB4fbBd/+QUQkdbW0uCHLtmWhkeK9YB
// uY05L1v1c6wcXI9IhSlBLnFFdxSTonAaZYhOusiG6eNFn7FpTU0i0oHcksQL+MW3HhbOnIyyUE1Wyjsm6pFuHKBi4TwHTQOibOhvxehuxyrHkqk7QcEPK6/ioN08n2eqd1mlfXiG2wk8nDaZfdmIq3hCm2usrpmqxJHYoH/wbbMeB7AzhreueWRk38984H2
// h1xX93ZpmUWJEJJGJ0St70Afb6RPjH9pX9vtbVXCvj65D+HPpxinReMBUj0rvGZ6IzNzoBhJYGszp5R4sdztGH8NLNWujwAFDThNRhWUX/r+APM1hdW9s=",
//       "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAaRdGrcBVugXTHUqmX0/UvgFUVjY/T1lnVQuz0gKhCE1aN0WvNPJautwNmv/68eAucdCm6vPqzg4KEgL777G5navyLj/1TMhLJfgTf1YNayDJLN7R13d1QQ3Peagcg+N4Itv0ZmZ6p7/QMQfw
// aXh30yronynPhxV//932ODigrZVrbW+XBhPtgh+/DlbCrU8d65IGn3VGTDQNfZaajmogy4xNf9x089OgcOv8H1XEjj0X8iZQwW+K05wIE6STWGxXJSMywMM7A+FE5YDrnEHeIT9bsIeXvEAy2cwfrorAnQrbfPyZqSSHHQzGumhOYam1Cyz1oVUMAhJMOXR
// drsckPXQdy38cOw/2VNFCZnLDvJIdT9kL3fk+BX/tXUMdvmR0ATY1JiqjW1YPO8fpVaG+IrbUobxDUnrq5kSmK6Gi9WIF+NzCWpPD/bjV+6nwJXEUxzjb18wVitZJZsQpMVsB9t0KXzlvr9AsKob4AZZGAAqbI4cHKjW3PNMbpH6U1WTUPldy7NQvpWDcYs
// VbYQOEr9YiKGyUPIUdb+nPSyAd4aIpbce8fQhN9D9wE0SIDTm2hMorjJsemwdZSHifZbL4Yya7QR/Oa3x5K+82IZiuhm/y5HGEdlHB2Jg54wqJJrKvO2E=",
//       "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAIt3G6MztjqXhUJ6Mbw/14mlqVbzIwJNU38ITmjBXACSyjgvQCMhVZYrvWQxCuEtPHboM1HrI1rgVjMC3B7vregAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
//     ],
//     "namespace_id": "AAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAow==",
//     "namespace_version": 0,
//     "share_proofs": [
//       {
//         "end": 2,
//         "nodes": [
//           "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABCU0aUrR/wpx09HFWeoyuV1vuw5Ew3rhtCaf/Zd4chb9",
//           "/////////////////////////////////////////////////////////////////////////////ypPU4ZqDz1t8YcunXI8ETuBth1gXLvPWIMd0JPoeJF3"
//         ],
//         "start": 1
//       },
//       {
//         "end": 2,
//         "nodes": [
//           "/////////////////////////////////////////////////////////////////////////////wdXw/2tc8hhuGLcsfU9pWo5BDIKSsNJFCytj++xtFgq"
//         ]
//       }
//     ]
//   },
//   "Shares": [
//     "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMBAAAEAAK7jTduoBTIVHIsXZYBTeXT+ROAP0ErS1wBn3qRFHoNClY8r4gEOLhvoPDfYX5dN+qGDHdIFPG4F1aF+niSmbfQRSkw2QdjqKwDKhYUKvu10oUo5r/k0SyYJx5KImSJ0d2sBH/ajcpk+D
// WBD0tXJTmsfiATmM8BqaxRRa5biE1T9yV1WKndAyJUC00P8e2/MG+7P1t7a9tMjG+Oxgxx1EzxJ47FDiRwnYNz2/JBqzIC33fKoiWZSeL+NFLn0Dfx+Ev1GYaKpstd1x1tgJnkEceTFVC6r7qhqRbTFJjAjgYJAB4fbBd/+QUQkdbW0uCHLtmWhkeK9YBuY
// 05L1v1c6wcXI9IhSlBLnFFdxSTonAaZYhOusiG6eNFn7FpTU0i0oHcksQL+MW3HhbOnIyyUE1Wyjsm6pFuHKBi4TwHTQOibOhvxehuxyrHkqk7QcEPK6/ioN08n2eqd1mlfXiG2wk8nDaZfdmIq3hCm2usrpmqxJHYoH/wbbMeB7AzhreueWRk38984H2h1
// xX93ZpmUWJEJJGJ0St70Afb6RPjH9pX9vtbVXCvj65D+HPpxinReMBUj0rvGZ6IzNzoBhJYGszp5R4sdztGH8NLNWujwAFDThNRhWUX/r+APM1hdW9s=",
//     "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAaRdGrcBVugXTHUqmX0/UvgFUVjY/T1lnVQuz0gKhCE1aN0WvNPJautwNmv/68eAucdCm6vPqzg4KEgL777G5navyLj/1TMhLJfgTf1YNayDJLN7R13d1QQ3Peagcg+N4Itv0ZmZ6p7/QMQfwaX
// h30yronynPhxV//932ODigrZVrbW+XBhPtgh+/DlbCrU8d65IGn3VGTDQNfZaajmogy4xNf9x089OgcOv8H1XEjj0X8iZQwW+K05wIE6STWGxXJSMywMM7A+FE5YDrnEHeIT9bsIeXvEAy2cwfrorAnQrbfPyZqSSHHQzGumhOYam1Cyz1oVUMAhJMOXRdr
// sckPXQdy38cOw/2VNFCZnLDvJIdT9kL3fk+BX/tXUMdvmR0ATY1JiqjW1YPO8fpVaG+IrbUobxDUnrq5kSmK6Gi9WIF+NzCWpPD/bjV+6nwJXEUxzjb18wVitZJZsQpMVsB9t0KXzlvr9AsKob4AZZGAAqbI4cHKjW3PNMbpH6U1WTUPldy7NQvpWDcYsVb
// YQOEr9YiKGyUPIUdb+nPSyAd4aIpbce8fQhN9D9wE0SIDTm2hMorjJsemwdZSHifZbL4Yya7QR/Oa3x5K+82IZiuhm/y5HGEdlHB2Jg54wqJJrKvO2E=",
//     "AAAAAAAAAAAAAAAAAAAAAAAAANjLtTOiQmHEwKMAIt3G6MztjqXhUJ6Mbw/14mlqVbzIwJNU38ITmjBXACSyjgvQCMhVZYrvWQxCuEtPHboM1HrI1rgVjMC3B7vregAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
// AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
//   ]
// }
// {
//   "header": {
//     "version": {
//       "block": "11",
//       "app": "1"
//     },
//     "chain_id": "private",
//     "height": "7621",
//     "time": "2024-08-29T13:06:07.863632032Z",
//     "last_block_id": {
//       "hash": "A2EDE7E806EBF59F1E2914AC2D7801B76B3A789F033911AF14C5D62F5C3E7098",
//       "parts": {
//         "total": 1,
//         "hash": "0C45BA8F78E4D8163C992AB47D7930583D1EF207D5C497FC59CC02281F625FEB"
//       }
//     },
//     "last_commit_hash": "526DB4A98D3661E05DF67B87239EC153515ACCD3F46BA268EFF4015295C6A2B3",
//     "data_hash": "E414A1EEB8395CA0C353384A9EC77300698ABBF98CD1DEC688A2C5BC5525077C",
//     "validators_hash": "73AE7622856703136CCEC52530EAEFB7BE02441ADC3395B469D82D13C1CD731F",
//     "next_validators_hash": "73AE7622856703136CCEC52530EAEFB7BE02441ADC3395B469D82D13C1CD731F",
//     "consensus_hash": "C0B6A634B72AE9687EA53B6D277A73ABA1386BA3CFC6D0F26963602F7F6FFCD6",
//     "app_hash": "5A6329D9A806308D54C1A28EA0D89A5B2F4AC206BA2F3E5BF8C5E9B15F65CC2F",
//     "last_results_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
//     "evidence_hash": "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
//     "proposer_address": "ECCF605776310B73EC57189AE89B1E587DBC2601"
//   },
//   "commit": {
//     "height": 7621,
//     "round": 0,
//     "block_id": {
//       "hash": "A7A4A45F0C655DA558722A14A589BA580F8A06BEE12376772A4CCFA30A989877",
//       "parts": {
//         "total": 1,
//         "hash": "617CFA4E6A5670FFCAF4D06FE72D16214FBAF3B7C96CA4FBD10B47A167B066D9"
//       }
//     },
//     "signatures": [
//       {
//         "block_id_flag": 2,
//         "validator_address": "ECCF605776310B73EC57189AE89B1E587DBC2601",
//         "timestamp": "2024-08-29T13:06:08.875333629Z",
//         "signature": "Beqnt4uw2reTFRguBXuQXMsEoysvUVlB4eyHBKJK6knigN478JUaKWfgXnuFyIA9XRdTpkiS/3OkqWimQXFfCQ=="
//       }
//     ]
//   },
//   "validator_set": {
//     "validators": [
//       {
//         "address": "ECCF605776310B73EC57189AE89B1E587DBC2601",
//         "pub_key": {
//           "type": "tendermint/PubKeyEd25519",
//           "value": "SIQRxnQENgPMQ8Vg1XBKtFEBLpqU4+hcxwrj6R3ZJp8="
//         },
//         "voting_power": "5000",
//         "proposer_priority": "0"
//       }
//     ],
//     "proposer": {
//       "address": "ECCF605776310B73EC57189AE89B1E587DBC2601",
//       "pub_key": {
//         "type": "tendermint/PubKeyEd25519",
//         "value": "SIQRxnQENgPMQ8Vg1XBKtFEBLpqU4+hcxwrj6R3ZJp8="
//       },
//       "voting_power": "5000",
//       "proposer_priority": "0"
//     }
//   },
//
