use blockstore::block::{Block, CidError};
use celestia_proto::share::p2p::shrex::nd::NamespaceRowResponse as RawNamespacedRow;
use cid::CidGeneric;
use multihash::Multihash;
use nmt_rs::simple_merkle::tree::MerkleHash;
use nmt_rs::NamespaceMerkleHasher;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::consts::appconsts;
use crate::nmt::{
    Namespace, NamespaceProof, NamespacedSha2Hasher, NMT_CODEC, NMT_ID_SIZE, NMT_MULTIHASH_CODE,
    NS_SIZE,
};
use crate::{Error, Result};

mod info_byte;

pub use info_byte::InfoByte;

/// A collection of rows of [`Share`]s from a particular [`Namespace`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawNamespacedShares", into = "RawNamespacedShares")]
pub struct NamespacedShares {
    /// All rows containing shares within some namespace.
    pub rows: Vec<NamespacedRow>,
}

/// [`Share`]s from a particular [`Namespace`] with proof in the data square row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawNamespacedRow", into = "RawNamespacedRow")]
pub struct NamespacedRow {
    /// All shares within some namespace in the given row.
    pub shares: Vec<Share>,
    /// A merkle proof of inclusion or absence of the shares in this row.
    pub proof: NamespaceProof,
}

/// A single fixed-size chunk of data which is used to form an [`ExtendedDataSquare`].
///
/// All data in Celestia is split into the [`Share`]s before being put into the
/// block's data square. See [`Blob::to_shares`].
///
/// All shares have the fixed size of 512 bytes and the following structure:
///
/// ```text
/// | Namespace | InfoByte | (optional) sequence length | data |
/// ```
///
/// The `sequence length` field indicates the byte length of the data split into shares.
/// If the data split into shares cannot fit into a single share, then each following
/// share shouldn't have this field set.
///
/// [`ExtendedDataSquare`]: crate::rsmt2d::ExtendedDataSquare
/// [`Blob::to_shares`]: crate::Blob::to_shares
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawShare", into = "RawShare")]
pub struct Share {
    /// A raw data of the share.
    pub data: [u8; appconsts::SHARE_SIZE],
}

impl Share {
    /// Create a new [`Share`] from the raw bytes.
    ///
    /// # Errors
    ///
    /// This function will return an error if the slice length is different than
    /// [`SHARE_SIZE`] or if the namespace is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use celestia_types::Share;
    ///
    /// let raw = [0; 512];
    /// let share = Share::from_raw(&raw).unwrap();
    /// ```
    ///
    /// [`SHARE_SIZE`]: crate::consts::appconsts::SHARE_SIZE
    pub fn from_raw(data: &[u8]) -> Result<Self> {
        if data.len() != appconsts::SHARE_SIZE {
            return Err(Error::InvalidShareSize(data.len()));
        }

        // validate the namespace to later return it
        // with the `new_unchecked`
        Namespace::from_raw(&data[..NS_SIZE])?;

        Ok(Share {
            data: data.try_into().unwrap(),
        })
    }

    /// Get the [`Namespace`] the [`Share`] belongs to.
    pub fn namespace(&self) -> Namespace {
        Namespace::new_unchecked(self.data[..NS_SIZE].try_into().unwrap())
    }

    /// Get all the data that follows the [`Namespace`] of the [`Share`].
    ///
    /// This will include also the [`InfoByte`] and the `sequence length`.
    pub fn data(&self) -> &[u8] {
        &self.data[NS_SIZE..]
    }

    /// Converts this [`Share`] into the raw bytes vector.
    ///
    /// This will include also the [`InfoByte`] and the `sequence length`.
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_ref().to_vec()
    }
}

impl AsRef<[u8]> for Share {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Block<NMT_ID_SIZE> for Share {
    fn cid(&self) -> Result<CidGeneric<NMT_ID_SIZE>, CidError> {
        let hasher = NamespacedSha2Hasher::with_ignore_max_ns(true);
        let digest = hasher.hash_leaf(self.as_ref()).iter().collect::<Vec<_>>();

        // size is correct, so unwrap is safe
        let mh = Multihash::wrap(NMT_MULTIHASH_CODE, &digest).unwrap();

        Ok(CidGeneric::new_v1(NMT_CODEC, mh))
    }

    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct RawShare {
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    data: Vec<u8>,
}

impl TryFrom<RawShare> for Share {
    type Error = Error;

    fn try_from(value: RawShare) -> Result<Self, Self::Error> {
        Share::from_raw(&value.data)
    }
}

impl From<Share> for RawShare {
    fn from(value: Share) -> Self {
        RawShare {
            data: value.to_vec(),
        }
    }
}

impl Protobuf<RawNamespacedRow> for NamespacedRow {}

impl TryFrom<RawNamespacedRow> for NamespacedRow {
    type Error = Error;

    fn try_from(value: RawNamespacedRow) -> Result<Self, Self::Error> {
        let shares = value
            .shares
            .into_iter()
            .map(|bytes| Share::from_raw(&bytes))
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

impl From<NamespacedRow> for RawNamespacedRow {
    fn from(value: NamespacedRow) -> RawNamespacedRow {
        RawNamespacedRow {
            shares: value.shares.iter().map(|share| share.to_vec()).collect(),
            proof: Some(value.proof.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmt::{NamespacedHash, NAMESPACED_HASH_SIZE};
    use base64::prelude::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn share_should_have_correct_len() {
        Share::from_raw(&[0; 0]).unwrap_err();
        Share::from_raw(&[0; 100]).unwrap_err();
        Share::from_raw(&[0; appconsts::SHARE_SIZE - 1]).unwrap_err();
        Share::from_raw(&[0; appconsts::SHARE_SIZE + 1]).unwrap_err();
        Share::from_raw(&[0; 2 * appconsts::SHARE_SIZE]).unwrap_err();

        Share::from_raw(&vec![0; appconsts::SHARE_SIZE]).unwrap();
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
            "is_max_namespace_ignored": true
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

        let nmt_rs::NamespaceProof::AbsenceProof {
            leaf: Some(leaf), ..
        } = &*proof
        else {
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
            "shares": [
              "AAAAAAAAAAAAAAAAAAAAAAAAAAAADCBNOWAP3dMBAAAAG/HyDKgAfpEKO/iy5h2g8mvKB+94cXpupUFl9QAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            ],
            "proof": {
              "start": 1,
              "end": 2,
              "nodes": [
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABFmTiyJVvgoyHdw7JGii/wyMfMbSdN3Nbi6Uj0Lcprk+",
                "/////////////////////////////////////////////////////////////////////////////0WE8jz9lbFjpXWj9v7/QgdAxYEqy4ew9TMdqil/UFZm"
              ],
              "leaf_hash": null,
              "is_max_namespace_ignored": true
            }
          }
        ]"#;

        let ns_shares: NamespacedShares =
            serde_json::from_str(get_shares_by_namespace_response).unwrap();

        assert_eq!(ns_shares.rows[0].shares.len(), 1);
        assert!(!ns_shares.rows[0].proof.is_of_absence());
    }

    #[test]
    fn test_generate_leaf_multihash() {
        let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
        let mut data = [0xCDu8; appconsts::SHARE_SIZE];
        data[..NS_SIZE].copy_from_slice(namespace.as_bytes());
        let share = Share::from_raw(&data).unwrap();

        let cid = share.cid().unwrap();
        assert_eq!(cid.codec(), NMT_CODEC);
        let hash = cid.hash();
        assert_eq!(hash.code(), NMT_MULTIHASH_CODE);
        assert_eq!(hash.size(), NAMESPACED_HASH_SIZE as u8);
        let hash = NamespacedHash::try_from(hash.digest()).unwrap();
        assert_eq!(hash.min_namespace(), *namespace);
        assert_eq!(hash.max_namespace(), *namespace);
    }
}
