use std::ops::{Deref, DerefMut};

use celestia_proto::share::p2p::shrex::nd::Proof as RawProof;
use nmt_rs::simple_merkle::proof::Proof as NmtProof;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::nmt::{to_nmt_namespaced_hash, NamespacedSha2Hasher, NS_SIZE};
use crate::{Error, Result};

type NmtNamespaceProof = nmt_rs::nmt_proof::NamespaceProof<NamespacedSha2Hasher, NS_SIZE>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawProof", into = "RawProof")]
pub struct NamespaceProof(NmtNamespaceProof);

impl Deref for NamespaceProof {
    type Target = NmtNamespaceProof;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NamespaceProof {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<NamespaceProof> for NmtNamespaceProof {
    fn from(value: NamespaceProof) -> NmtNamespaceProof {
        value.0
    }
}

impl From<NmtNamespaceProof> for NamespaceProof {
    fn from(value: NmtNamespaceProof) -> NamespaceProof {
        NamespaceProof(value)
    }
}

impl Protobuf<RawProof> for NamespaceProof {}

impl TryFrom<RawProof> for NamespaceProof {
    type Error = Error;

    fn try_from(value: RawProof) -> Result<Self, Self::Error> {
        let siblings = value
            .nodes
            .iter()
            .map(|bytes| to_nmt_namespaced_hash(bytes))
            .collect::<Result<Vec<_>>>()?;

        let mut proof = NmtNamespaceProof::PresenceProof {
            proof: NmtProof {
                siblings,
                start_idx: value.start as u32,
            },
            ignore_max_ns: true,
        };

        if !value.hashleaf.is_empty() {
            proof.convert_to_absence_proof(to_nmt_namespaced_hash(&value.hashleaf)?);
        }

        Ok(NamespaceProof(proof))
    }
}

impl From<NamespaceProof> for RawProof {
    fn from(_value: NamespaceProof) -> Self {
        todo!();
    }
}
