use celestia_tendermint::crypto::default::Sha256;
use celestia_tendermint::merkle::{Hash, MerkleHash};
use celestia_tendermint_proto::{v0_34::crypto::Proof as RawMerkleProof, Protobuf};
use serde::{Deserialize, Serialize};

use crate::{
    bail_validation, bail_verification, validation_error, verification_error, Error, Result,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawMerkleProof", into = "RawMerkleProof")]
pub struct MerkleProof {
    index: usize,
    total: usize,
    leaf_hash: Hash,
    aunts: Vec<Hash>,
}

impl MerkleProof {
    pub fn new(leaf_to_prove: usize, leaves: &[impl AsRef<[u8]>]) -> Result<(Self, Hash)> {
        let total = leaves.len().next_power_of_two();
        if leaf_to_prove >= leaves.len() {
            // todo, error
            bail_validation!("leaf index out of bounds");
        }

        let mut hasher = Sha256::default();
        let tree_height = total.ilog2() as usize;

        // create lowest level of the tree, padding with empty hashes
        let mut tree_level: Vec<_> = leaves
            .iter()
            .map(|leaf| hasher.leaf_hash(leaf.as_ref()))
            .collect();
        tree_level.resize(total, hasher.empty_hash());

        // save the leaf we're about to prove
        let proven_leaf = tree_level[leaf_to_prove];
        let mut aunts = Vec::with_capacity(tree_height);
        let mut current_leaf = leaf_to_prove;

        for _ in 0..tree_height {
            // the sibling node will be used in proof to reconstruct root
            let sibling = current_leaf ^ 1;
            aunts.push(tree_level[sibling]);

            // construct higher tree level
            tree_level = tree_level
                .chunks(2)
                .map(|pair| hasher.inner_hash(pair[0], pair[1]))
                .collect();
            current_leaf /= 2;
        }

        // last tree level is just root
        debug_assert_eq!(tree_level.len(), 1);
        let root = tree_level[0];
        let proof = Self {
            index: leaf_to_prove,
            total,
            leaf_hash: proven_leaf,
            aunts,
        };

        Ok((proof, root))
    }

    pub fn verify(&self, leaf: impl AsRef<[u8]>, root: Hash) -> Result<()> {
        let mut hasher = Sha256::default();
        let leaf = hasher.leaf_hash(leaf.as_ref());

        if leaf != self.leaf_hash {
            return Err(verification_error!("proof created for a different leaf").into());
        }

        let computed_root = subtree_root_from_aunts(self.index, self.total, leaf, &self.aunts)?;

        if computed_root != root {
            return Err(Error::RootMismatch);
        }

        Ok(())
    }
}

// aunts are effectively a merkle proof for the leaf, so inner hashes needed
// to recompute root hash of a merkle tree
fn subtree_root_from_aunts(index: usize, total: usize, leaf: Hash, aunts: &[Hash]) -> Result<Hash> {
    debug_assert_ne!(total, 0);

    let root = if total == 1 {
        // we reached the leaf
        if !aunts.is_empty() {
            bail_verification!("extra aunts in proof");
        }
        leaf
    } else {
        let mut hasher = Sha256::default();
        let subtrees_split = total.next_power_of_two() / 2;

        // take next subtree root's sibling
        let (sibling, aunts) = aunts
            .split_last()
            .ok_or_else(|| verification_error!("aunts missing in proof"))?;

        if index < subtrees_split {
            // and recurse into left subtree
            let left_hash = subtree_root_from_aunts(index, subtrees_split, leaf, aunts)?;
            hasher.inner_hash(left_hash, *sibling)
        } else {
            // and recurse into right subtree
            let right_hash = subtree_root_from_aunts(
                index - subtrees_split,
                total - subtrees_split,
                leaf,
                aunts,
            )?;
            hasher.inner_hash(*sibling, right_hash)
        }
    };

    Ok(root)
}

impl Protobuf<RawMerkleProof> for MerkleProof {}

impl TryFrom<RawMerkleProof> for MerkleProof {
    type Error = Error;

    fn try_from(value: RawMerkleProof) -> Result<Self, Self::Error> {
        if value.index < 0 {
            bail_validation!("negative index");
        }
        if value.total <= 0 {
            bail_validation!("total <= 0");
        }
        Ok(Self {
            index: value.index as usize,
            total: value.total as usize,
            leaf_hash: value
                .leaf_hash
                .try_into()
                .map_err(|_| validation_error!("invalid hash size"))?,
            aunts: value
                .aunts
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()
                .map_err(|_| validation_error!("invalid hash size"))?,
        })
    }
}

impl From<MerkleProof> for RawMerkleProof {
    fn from(value: MerkleProof) -> Self {
        Self {
            index: value.index as i64,
            total: value.total as i64,
            leaf_hash: value.leaf_hash.into(),
            aunts: value.aunts.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::random_bytes;

    use super::MerkleProof;

    #[test]
    fn create_and_verify() {
        for _ in 0..5 {
            let leaf_size = (rand::random::<usize>() % 1024) + 1;
            let leaves_amount = (rand::random::<usize>() % 128) + 1;
            let data = random_bytes(leaves_amount * leaf_size);

            let leaves: Vec<_> = data.chunks(leaf_size).collect();
            let leaf_to_prove = rand::random::<usize>() % leaves_amount;

            let (proof, root) = MerkleProof::new(leaf_to_prove, &leaves).unwrap();

            proof.verify(leaves[leaf_to_prove], root).unwrap();
            proof.verify(random_bytes(leaf_size), root).unwrap_err();
            proof
                .verify(leaves[leaf_to_prove], rand::random())
                .unwrap_err();
        }
    }
}
