use celestia_proto::cosmos::base::tendermint::v1beta1::{ProofOp, ProofOps};
use ics23::commitment_proof::Proof;
use ics23::{CommitmentProof, ExistenceProof};
use prost::Message;
use tendermint::crypto::default::Sha256;
use tendermint::crypto::Sha256 as _;

/// Representation of all the errors that can occur when trying to read and verify ABCI proof
#[derive(Debug, thiserror::Error)]
pub enum ProofError {
    // NOTE: we don't get the computed root from ics23, we would need to contribute to it
    #[error("Computed root is different than expected")]
    RootMismatch,

    #[error("ABCI proof missing")]
    AbciProofMissing,

    #[error("Unknown proof spec: {0}")]
    UnknownSpec(String),

    #[error("Decoding from proto failed: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("Existance proof missing in operation")]
    ExistanceProofMissing,

    #[error("Operation key ({0:?}) is different than expected ({1:?})")]
    OperationKeyMismatch(Vec<u8>, Vec<u8>),
}

#[derive(Debug)]
struct CommitmentOp {
    key: Vec<u8>,
    spec: ics23::ProofSpec,
    proof: ics23::CommitmentProof,
}

impl TryFrom<ProofOp> for CommitmentOp {
    type Error = ProofError;

    fn try_from(value: ProofOp) -> Result<Self, Self::Error> {
        let spec = match value.r#type.as_str() {
            "ics23:iavl" => ics23::iavl_spec(),
            "ics23:simple" => ics23::tendermint_spec(),
            _ => return Err(ProofError::UnknownSpec(value.r#type.clone())),
        };
        Ok(Self {
            key: value.key,
            spec,
            proof: CommitmentProof::decode(value.data.as_slice())?,
        })
    }
}

impl CommitmentOp {
    fn get_existence_proof(&self, key: impl AsRef<[u8]>) -> Option<&ExistenceProof> {
        match self.proof.proof.as_ref()? {
            Proof::Exist(proof) => Some(proof),
            Proof::Batch(batch) => batch.entries.iter().find_map(|entry| {
                if let ics23::batch_entry::Proof::Exist(proof) = entry.proof.as_ref()? {
                    if key.as_ref() == proof.key {
                        Some(proof)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }),
            _ => None,
        }
    }
}

pub struct ProofsChain(Vec<CommitmentOp>);

impl ProofsChain {
    pub fn verify_membership(
        &self,
        root: impl AsRef<[u8]>,
        keys: impl IntoIterator<Item = impl AsRef<[u8]>>,
        leaf: impl AsRef<[u8]>,
    ) -> Result<(), ProofError> {
        let root = root.as_ref();
        let mut current_leaf = leaf.as_ref();

        for (i, key) in keys.into_iter().enumerate() {
            let key = key.as_ref();
            let proof = self.0.get(i).unwrap();

            if key != proof.key {
                return Err(ProofError::OperationKeyMismatch(
                    proof.key.clone(),
                    key.to_vec(),
                ));
            }

            if proof.get_existence_proof(key).is_none() {
                return Err(ProofError::ExistanceProofMissing);
            }

            // current proof must prove current leaf to the root of the current tree,
            // which is at the same time the leaf for the next proof or the uppermost root
            // in case we are in proving the last tree
            let current_root = self
                .0
                .get(i + 1)
                .and_then(|proof| {
                    proof
                        .get_existence_proof(key)
                        .map(|proof| proof.value.as_slice())
                })
                .unwrap_or(root);

            if !ics23::verify_membership::<Sha256Provider>(
                &proof.proof,
                &proof.spec,
                &current_root.to_vec(), // needs fix upstream
                &proof.key,
                current_leaf,
            ) {
                return Err(ProofError::RootMismatch);
            }

            current_leaf = current_root;
        }
        Ok(())
    }
}

impl TryFrom<ProofOps> for ProofsChain {
    type Error = ProofError;

    fn try_from(value: ProofOps) -> Result<Self, Self::Error> {
        if value.ops.is_empty() {
            return Err(ProofError::AbciProofMissing);
        }
        let chain = value
            .ops
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?;

        Ok(Self(chain))
    }
}

struct Sha256Provider;

impl ics23::HostFunctionsProvider for Sha256Provider {
    fn sha2_256(message: &[u8]) -> [u8; 32] {
        Sha256::digest(message)
    }

    fn sha2_512(_: &[u8]) -> [u8; 64] {
        [0; 64]
    }

    fn sha2_512_truncated(_: &[u8]) -> [u8; 32] {
        [0; 32]
    }

    fn keccak_256(_: &[u8]) -> [u8; 32] {
        [0; 32]
    }

    fn ripemd160(_: &[u8]) -> [u8; 20] {
        [0; 20]
    }

    fn blake2b_512(_: &[u8]) -> [u8; 64] {
        [0; 64]
    }

    fn blake2s_256(_: &[u8]) -> [u8; 32] {
        [0; 32]
    }

    fn blake3(_: &[u8]) -> [u8; 32] {
        [0; 32]
    }
}
