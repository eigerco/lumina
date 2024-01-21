use core::ops::{Deref, DerefMut};

use celestia_proto::proof::pb::Proof as RawProof;
use celestia_tendermint_proto::Protobuf;
use nmt_rs::simple_merkle::proof::Proof as NmtProof;
use serde::{Deserialize, Serialize};

use crate::nmt::{NamespacedHash, NamespacedHashExt, NamespacedSha2Hasher, NS_SIZE};
use crate::types::Vec;
use crate::{Error, Result};

type NmtNamespaceProof = nmt_rs::nmt_proof::NamespaceProof<NamespacedSha2Hasher, NS_SIZE>;

/// A helper constant to be used as leaves when verifying the [`NamespaceProof`] of absence.
pub const EMPTY_LEAVES: &[&[u8]] = &[];

/// Merkle proof of inclusion or absence of some data in the [`Nmt`].
///
/// # Example
///
/// ```
/// use nmt_rs::NamespaceMerkleHasher;
/// use celestia_types::nmt::{Namespace, Nmt, NamespacedSha2Hasher, EMPTY_LEAVES};
///
/// let ns1 = Namespace::new_v0(&[1]).unwrap();
/// let ns2 = Namespace::new_v0(&[2]).unwrap();
/// let ns3 = Namespace::new_v0(&[3]).unwrap();
/// let ns4 = Namespace::new_v0(&[4]).unwrap();
///
/// let leaves = [
///     (ns1, b"leaf0"),
///     (ns2, b"leaf1"),
///     (ns2, b"leaf2"),
///     (ns4, b"leaf3"),
/// ];
///
/// // create the nmt and feed it with data
/// let mut nmt = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
///
/// for (namespace, data) in leaves {
///     nmt.push_leaf(data, *namespace);
/// }
///
/// // create and verify the proof of inclusion of namespace 2 data
/// let root = nmt.root();
/// let proof = nmt.get_namespace_proof(*ns2);
/// assert!(proof.is_of_presence());
/// assert!(
///     proof.verify_complete_namespace(&root, &["leaf1", "leaf2"], *ns2).is_ok()
/// );
///
/// // create and verify the proof of absence of namespace 3 data
/// let proof = nmt.get_namespace_proof(*ns3);
/// assert!(proof.is_of_absence());
/// assert!(
///     proof.verify_complete_namespace(&root, EMPTY_LEAVES, *ns3).is_ok()
/// );
/// ```
///
/// [`Nmt`]: crate::nmt::Nmt
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawProof", into = "RawProof")]
pub struct NamespaceProof(NmtNamespaceProof);

impl NamespaceProof {
    /// Convert the proof to the underlying [`nmt_rs`] equivalent.
    pub fn into_inner(self) -> NmtNamespaceProof {
        self.0
    }

    /// Get the hash of the leaf following the [`Namespace`] which absence is being proven.
    ///
    /// If the tree had contained the proven namespace, it should be in the tree
    /// right before the leaf returned by this function.
    ///
    /// This function returns [`None`] if the proof isn't an [`AbsenceProof`] or the
    /// proven [`Namespace`] is not in the range of the tree root [`NamespacedHash`].
    ///
    /// [`Namespace`]: crate::nmt::Namespace
    /// [`AbsenceProof`]: NmtNamespaceProof::PresenceProof
    /// [`NamespacedHash`]: crate::nmt::NamespacedHash
    pub fn leaf(&self) -> Option<&NamespacedHash> {
        match &self.0 {
            NmtNamespaceProof::AbsenceProof { leaf, .. } => leaf.as_ref(),
            _ => None,
        }
    }

    /// Returns true if the proof ignores all the leaves inserted with
    /// [`Namespace::PARITY_SHARE`].
    ///
    /// [`Namespace::PARITY_SHARE`]: crate::nmt::Namespace::PARITY_SHARE
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
