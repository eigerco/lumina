use celestia_proto::share::p2p::shrex::nd::{Proof as RawProof, Row as RawRow};
use nmt_rs::simple_merkle::proof::Proof as NmtProof;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::nmt::{Namespace, NamespaceProof, NamespacedHash, NS_SIZE};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NamespacedShares {
    pub rows: Vec<NamespacedRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawRow", into = "RawRow")]
pub struct NamespacedRow {
    pub shares: Vec<Share>,
    pub proof: NamespaceProof,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Share {
    pub namespace: Namespace,
    pub data: Vec<u8>,
}

impl Share {
    fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < NS_SIZE {
            return Err(Error::InvalidShareSize(bytes.len()));
        }

        let (ns, data) = bytes.split_at(NS_SIZE);

        Ok(Share {
            namespace: Namespace::from_raw(ns)?,
            data: data.to_vec(),
        })
    }

    fn _to_vec(&self) -> Vec<u8> {
        let mut bytes = self.namespace.as_bytes().to_vec();
        bytes.extend_from_slice(&self.data);
        bytes
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

        let proof = value
            .proof
            .map(proof_from_proto)
            .transpose()?
            .ok_or(Error::MissingProof)?;

        Ok(NamespacedRow { shares, proof })
    }
}

impl From<NamespacedRow> for RawRow {
    fn from(_value: NamespacedRow) -> RawRow {
        todo!();
    }
}

fn proof_from_proto(raw_proof: RawProof) -> Result<NamespaceProof> {
    let siblings = raw_proof
        .nodes
        .iter()
        .map(|n| to_namespaced_hash(n))
        .collect::<Result<Vec<_>>>()?;

    let mut proof = NamespaceProof::PresenceProof {
        proof: NmtProof {
            siblings,
            start_idx: raw_proof.start as u32,
        },
        ignore_max_ns: true,
    };

    if !raw_proof.hashleaf.is_empty() {
        proof.convert_to_absence_proof(to_namespaced_hash(&raw_proof.hashleaf)?);
    }

    Ok(proof)
}

fn to_namespaced_hash(node: &[u8]) -> Result<NamespacedHash> {
    node.try_into().map_err(|_| Error::InvalidNamespacedHash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::*;

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

        let raw_proof: RawProof =
            serde_json::from_str(blob_get_proof_response).expect("can not parse proof");
        let proof = proof_from_proto(raw_proof).expect("proof_from_proto failed");
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

    fn b64_decode(s: &str) -> Vec<u8> {
        BASE64_STANDARD.decode(s).expect("failed to decode base64")
    }
}
