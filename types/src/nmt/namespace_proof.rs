use std::ops::{Deref, DerefMut};

use celestia_proto::proof::pb::Proof as RawProof;
use nmt_rs::simple_merkle::proof::Proof as NmtProof;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::nmt::{NamespacedHash, NamespacedHashExt, NamespacedSha2Hasher, NS_SIZE};
use crate::{Error, Result};

pub type NmtNamespaceProof = nmt_rs::nmt_proof::NamespaceProof<NamespacedSha2Hasher, NS_SIZE>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawProof", into = "RawProof")]
pub struct NamespaceProof(NmtNamespaceProof);

impl NamespaceProof {
    pub fn into_inner(self) -> NmtNamespaceProof {
        self.0
    }

    pub fn leaf(&self) -> Option<&NamespacedHash> {
        match &self.0 {
            NmtNamespaceProof::AbsenceProof { leaf, .. } => leaf.as_ref(),
            _ => None,
        }
    }

    pub fn max_ns_ignored(&self) -> bool {
        match &self.0 {
            NmtNamespaceProof::AbsenceProof { ignore_max_ns, .. }
            | NmtNamespaceProof::PresenceProof { ignore_max_ns, .. } => *ignore_max_ns,
        }
    }
}

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
            .map(|bytes| NamespacedHash::from_raw(bytes))
            .collect::<Result<Vec<_>>>()?;

        let mut proof = NmtNamespaceProof::PresenceProof {
            proof: NmtProof {
                siblings,
                range: value.start as u32..value.end as u32,
            },
            ignore_max_ns: value.is_max_namespace_ignored,
        };

        if !value.leaf_hash.is_empty() {
            proof.convert_to_absence_proof(NamespacedHash::from_raw(&value.leaf_hash)?);
        }

        Ok(NamespaceProof(proof))
    }
}

impl From<NamespaceProof> for RawProof {
    fn from(value: NamespaceProof) -> Self {
        RawProof {
            start: value.start_idx() as i64,
            end: value.end_idx() as i64,
            nodes: value.siblings().iter().map(|hash| hash.to_vec()).collect(),
            leaf_hash: value.leaf().map(|hash| hash.to_vec()).unwrap_or_default(),
            is_max_namespace_ignored: value.max_ns_ignored(),
        }
    }
}
