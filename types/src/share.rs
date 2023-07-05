use celestia_proto::share::p2p::shrex::nd::Row as RawRow;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::consts::appconsts;
use crate::nmt::{Namespace, NamespaceProof, NS_SIZE};
use crate::{Error, Result};

mod info_byte;

pub use info_byte::InfoByte;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawNamespacedShares", into = "RawNamespacedShares")]
pub struct NamespacedShares {
    pub rows: Vec<NamespacedRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawRow", into = "RawRow")]
pub struct NamespacedRow {
    pub shares: Vec<Share>,
    pub proof: NamespaceProof,
}

// NOTE:
// Share ::= SHARE_SIZE bytes {
//      Namespace   NS_SIZE bytes
//      InfoByte    SHARE_INFO_BYTES bytes
//      SequenceLen SEQUENCE_LEN_BYTES bytes OPTIONAL
//      Data        bytes
// }
#[derive(Debug, Clone)]
pub struct Share {
    pub namespace: Namespace,
    pub data: Vec<u8>,
}

impl Share {
    fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != appconsts::SHARE_SIZE {
            return Err(Error::InvalidShareSize(bytes.len()));
        }

        let (ns, data) = bytes.split_at(NS_SIZE);

        Ok(Share {
            namespace: Namespace::from_raw(ns)?,
            data: data.to_vec(),
        })
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut bytes = self.namespace.as_bytes().to_vec();
        bytes.extend_from_slice(&self.data);
        bytes
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct RawNamespacedShares {
    rows: Option<Vec<NamespacedRow>>,
}

impl From<RawNamespacedShares> for NamespacedShares {
    fn from(value: RawNamespacedShares) -> Self {
        Self {
            rows: value.rows.unwrap_or_default(),
        }
    }
}

impl From<NamespacedShares> for RawNamespacedShares {
    fn from(value: NamespacedShares) -> Self {
        let rows = if value.rows.is_empty() {
            None
        } else {
            Some(value.rows)
        };

        Self { rows }
    }
}

impl Protobuf<RawRow> for NamespacedRow {}

impl TryFrom<RawRow> for NamespacedRow {
    type Error = Error;

    fn try_from(value: RawRow) -> Result<Self, Self::Error> {
        let shares = value
            .shares
            .into_iter()
            .map(Share::new)
            .collect::<Result<Vec<_>>>()?;

        let proof: NamespaceProof = value
            .proof
            .map(TryInto::try_into)
            .transpose()?
            .ok_or(Error::MissingProof)?;

        if shares.is_empty() && !proof.is_of_absence() {
            return Err(Error::WrongProofType);
        }

        Ok(NamespacedRow { shares, proof })
    }
}

impl From<NamespacedRow> for RawRow {
    fn from(value: NamespacedRow) -> RawRow {
        RawRow {
            shares: value.shares.iter().map(|share| share.to_vec()).collect(),
            proof: Some(value.proof.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::*;

    #[test]
    fn share_should_have_correct_len() {
        Share::new(vec![0; 0]).unwrap_err();
        Share::new(vec![0; 100]).unwrap_err();
        Share::new(vec![0; appconsts::SHARE_SIZE - 1]).unwrap_err();
        Share::new(vec![0; appconsts::SHARE_SIZE + 1]).unwrap_err();
        Share::new(vec![0; 2 * appconsts::SHARE_SIZE]).unwrap_err();

        Share::new(vec![0; appconsts::SHARE_SIZE]).unwrap();
    }

    #[test]
    fn decode_presence_proof() {
        let blob_get_proof_response = r#"{
            "start": 1,
            "end": 2,
            "nodes": [
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABA+poCQOx7UzVkteV9DgcA6g29ZXXOp0hYZb67hoNkFP",
                "/////////////////////////////////////////////////////////////////////////////8PbbPgQcFSaW2J/BWiJqrCoj6K4g/UUd0Y9dadwqrz+"
            ]
        }"#;

        let proof: NamespaceProof =
            serde_json::from_str(blob_get_proof_response).expect("can not parse proof");

        assert!(!proof.is_of_absence());

        let sibling = &proof.siblings()[0];
        let min_ns_bytes = &sibling.min_namespace().0[..];
        let max_ns_bytes = &sibling.max_namespace().0[..];
        let hash_bytes = &sibling.hash()[..];
        assert_eq!(
            min_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQ=")
        );
        assert_eq!(
            max_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQ=")
        );
        assert_eq!(
            hash_bytes,
            b64_decode("D6mgJA7HtTNWS15X0OBwDqDb1ldc6nSFhlvruGg2QU8=")
        );

        let sibling = &proof.siblings()[1];
        let min_ns_bytes = &sibling.min_namespace().0[..];
        let max_ns_bytes = &sibling.max_namespace().0[..];
        let hash_bytes = &sibling.hash()[..];
        assert_eq!(
            min_ns_bytes,
            b64_decode("//////////////////////////////////////8=")
        );
        assert_eq!(
            max_ns_bytes,
            b64_decode("//////////////////////////////////////8=")
        );
        assert_eq!(
            hash_bytes,
            b64_decode("w9ts+BBwVJpbYn8FaImqsKiPoriD9RR3Rj11p3CqvP4=")
        );
    }

    #[test]
    fn decode_absence_proof() {
        let blob_get_proof_response = r#"{
            "start": 1,
            "end": 2,
            "nodes": [
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABD+sL4GAQk9mj+ejzHmHjUJEyemkpExb+S5aEDtmuHEq",
                "/////////////////////////////////////////////////////////////////////////////zgUEBW/wWmCfnwXfalgqMfK9sMy168y3XRzdwY1jpZY"
            ],
            "leaf_hash": "AAAAAAAAAAAAAAAAAAAAAAAAAJLVUf6krS8362EAAAAAAAAAAAAAAAAAAAAAAAAAktVR/qStLzfrYeEAWUHOa+lE38pJyHstgGaqi9RXPhZtzUscK7iTUbQS",
            "is_max_namespace_id_ignored": true
        }"#;

        let proof: NamespaceProof =
            serde_json::from_str(blob_get_proof_response).expect("can not parse proof");

        assert!(proof.is_of_absence());

        let sibling = &proof.siblings()[0];
        let min_ns_bytes = &sibling.min_namespace().0[..];
        let max_ns_bytes = &sibling.max_namespace().0[..];
        let hash_bytes = &sibling.hash()[..];
        assert_eq!(
            min_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQ=")
        );
        assert_eq!(
            max_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQ=")
        );
        assert_eq!(
            hash_bytes,
            b64_decode("P6wvgYBCT2aP56PMeYeNQkTJ6aSkTFv5LloQO2a4cSo=")
        );

        let sibling = &proof.siblings()[1];
        let min_ns_bytes = &sibling.min_namespace().0[..];
        let max_ns_bytes = &sibling.max_namespace().0[..];
        let hash_bytes = &sibling.hash()[..];
        assert_eq!(
            min_ns_bytes,
            b64_decode("//////////////////////////////////////8=")
        );
        assert_eq!(
            max_ns_bytes,
            b64_decode("//////////////////////////////////////8=")
        );
        assert_eq!(
            hash_bytes,
            b64_decode("OBQQFb/BaYJ+fBd9qWCox8r2wzLXrzLddHN3BjWOllg=")
        );

        let nmt_rs::NamespaceProof::AbsenceProof { leaf: Some(leaf), .. } = &*proof else {
            unreachable!();
        };

        let min_ns_bytes = &leaf.min_namespace().0[..];
        let max_ns_bytes = &leaf.max_namespace().0[..];
        let hash_bytes = &leaf.hash()[..];
        assert_eq!(
            min_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAJLVUf6krS8362E=")
        );
        assert_eq!(
            max_ns_bytes,
            b64_decode("AAAAAAAAAAAAAAAAAAAAAAAAAJLVUf6krS8362E=")
        );
        assert_eq!(
            hash_bytes,
            b64_decode("4QBZQc5r6UTfyknIey2AZqqL1Fc+Fm3NSxwruJNRtBI=")
        );
    }

    fn b64_decode(s: &str) -> Vec<u8> {
        BASE64_STANDARD.decode(s).expect("failed to decode base64")
    }

    #[test]
    fn decode_namespaced_shares() {
        let get_shares_by_namespace_response = r#"[
          {
            "Shares": [
              "AAAAAAAAAAAAAAAAAAAAAAAAAAAADCBNOWAP3dMBAAAAG/HyDKgAfpEKO/iy5h2g8mvKB+94cXpupUFl9QAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            ],
            "Proof": {
              "start": 1,
              "end": 2,
              "nodes": [
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABFmTiyJVvgoyHdw7JGii/wyMfMbSdN3Nbi6Uj0Lcprk+",
                "/////////////////////////////////////////////////////////////////////////////0WE8jz9lbFjpXWj9v7/QgdAxYEqy4ew9TMdqil/UFZm"
              ],
              "leaf_hash": null,
              "is_max_namespace_id_ignored": true
            }
          }
        ]"#;

        let ns_shares: NamespacedShares =
            serde_json::from_str(get_shares_by_namespace_response).unwrap();

        assert_eq!(ns_shares.rows[0].shares.len(), 1);
        assert!(!ns_shares.rows[0].proof.is_of_absence());
    }
}
