use celestia_proto::celestia::core::v1::proof::ShareProof as RawShareProof;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::consts::appconsts::SHARE_SIZE;
use crate::hash::Hash;
use crate::nmt::NamespaceProof;
use crate::{bail_verification, validation_error, RowProof};
use crate::{nmt::Namespace, Error, Result};

/// A proof of inclusion of a continouous range of shares of some namespace
/// in a [`DataAvailabilityHeader`].
///
/// The proof will proof the inclusion of shares in row roots they span
/// and the inclusion of those row roots in the dah.
///
/// [`DataAvailabilityHeader`]: crate::DataAvailabilityHeader
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawShareProof", into = "RawShareProof")]
pub struct ShareProof {
    pub data: Vec<[u8; SHARE_SIZE]>,
    pub namespace_id: Namespace,
    pub share_proofs: Vec<NamespaceProof>,
    pub row_proof: RowProof,
}

impl ShareProof {
    /// Get the shares proven by this proof.
    pub fn shares(&self) -> &[[u8; SHARE_SIZE]] {
        &self.data
    }

    /// Verify the proof against the hash of [`DataAvailabilityHeader`], proving
    /// the inclusion of shares.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    ///  - the proof is malformed. Number of shares, nmt proofs and row proofs needs to match.
    ///  - the verification of any inner row proof fails
    ///
    /// [`DataAvailabilityHeader`]: crate::DataAvailabilityHeader
    pub fn verify(&self, root: Hash) -> Result<()> {
        let row_roots = self.row_proof.row_roots();

        if self.share_proofs.len() != row_roots.len() {
            bail_verification!(
                "share proofs length ({}) != row roots length ({})",
                self.share_proofs.len(),
                row_roots.len()
            );
        }

        let mut shares_needed = 0;
        for proof in &self.share_proofs {
            if proof.is_of_absence() {
                bail_verification!("only presence proofs allowed");
            }
            if proof.start_idx() >= proof.end_idx() {
                bail_verification!("proof without data");
            }

            shares_needed += proof.end_idx() - proof.start_idx();
        }

        if shares_needed as usize != self.data.len() {
            bail_verification!(
                "shares needed ({}) != proof's data length ({})",
                shares_needed,
                self.data.len()
            );
        }

        self.row_proof.verify(root)?;

        let mut data = self.data.as_slice();

        for (proof, row) in self.share_proofs.iter().zip(row_roots) {
            let amount = proof.end_idx() - proof.start_idx();
            let leaves = &data[..amount as usize];
            proof
                .verify_range(row, leaves, *self.namespace_id)
                .map_err(Error::RangeProofError)?;
            data = &data[amount as usize..];
        }

        Ok(())
    }
}

impl Protobuf<RawShareProof> for ShareProof {}

impl TryFrom<RawShareProof> for ShareProof {
    type Error = Error;

    fn try_from(value: RawShareProof) -> Result<Self> {
        Ok(Self {
            data: value
                .data
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()
                .map_err(|_| validation_error!("invalid share size"))?,
            namespace_id: Namespace::new(
                value
                    .namespace_version
                    .try_into()
                    .map_err(|_| validation_error!("namespace version must be single byte"))?,
                &value.namespace_id,
            )?,
            share_proofs: value
                .share_proofs
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_>>()?,
            row_proof: value
                .row_proof
                .ok_or_else(|| validation_error!("row proof missing"))?
                .try_into()?,
        })
    }
}

impl From<ShareProof> for RawShareProof {
    fn from(value: ShareProof) -> Self {
        Self {
            data: value.data.into_iter().map(Into::into).collect(),
            namespace_id: value.namespace_id.id().to_vec(),
            namespace_version: value.namespace_id.version() as u32,
            share_proofs: value.share_proofs.into_iter().map(Into::into).collect(),
            row_proof: Some(value.row_proof.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::DataAvailabilityHeader;

    use super::ShareProof;

    #[test]
    fn share_proof_serde() {
        let raw_share_proof = r#"{
          "data": [
            "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"
          ],
          "namespace_id": "AAAAAAAAAAAAAAAAAAAAAAAA2Mu1M6JCYcTAow==",
          "namespace_version": 0,
          "share_proofs": [
            {
              "end": 2,
              "nodes": [
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABCU0aUrR/wpx09HFWeoyuV1vuw5Ew3rhtCaf/Zd4chb9",
                "/////////////////////////////////////////////////////////////////////////////ypPU4ZqDz1t8YcunXI8ETuBth1gXLvPWIMd0JPoeJF3"
              ],
              "start": 1
            },
            {
              "end": 2,
              "nodes": [
                "/////////////////////////////////////////////////////////////////////////////wdXw/2tc8hhuGLcsfU9pWo5BDIKSsNJFCytj++xtFgq"
              ]
            }
          ],
          "row_proof": {
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
        }"#;
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

        let proof: ShareProof = serde_json::from_str(raw_share_proof).unwrap();
        let dah: DataAvailabilityHeader = serde_json::from_str(raw_dah).unwrap();

        proof.verify(dah.hash()).unwrap()
    }
}
