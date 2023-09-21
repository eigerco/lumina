use celestia_proto::share::eds::byzantine::pb::BadEncoding as RawBadEncodingFraudProof;
use celestia_proto::share::eds::byzantine::pb::MerkleProof as RawEdsProof;
use celestia_proto::share::eds::byzantine::pb::Share as RawShareWithProof;
use celestia_proto::share::p2p::shrex::nd::Proof as RawShrexProof;
use cid::multihash::MultihashGeneric;
use cid::CidGeneric;
use serde::{Deserialize, Serialize};
use tendermint::hash::SHA256_HASH_SIZE;
use tendermint::{block::Height, Hash};
use tendermint_proto::Protobuf;

use crate::bail_validation;
use crate::consts::appconsts;
use crate::fraud_proof::FraudProof;
use crate::nmt::NamespacedHash;
use crate::nmt::{Namespace, NamespaceProof, NamespacedHashExt, NS_SIZE};
use crate::rsmt2d::Axis;
use crate::{Error, ExtendedHeader, Result, Share};

pub const MULTIHASH_NMT_CODEC_CODE: u64 = 0x7700;
pub const MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE: u64 = 0x7701;
pub const MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE: usize = 2 * NS_SIZE + SHA256_HASH_SIZE;

type Cid = CidGeneric<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE>;
type Multihash = MultihashGeneric<MULTIHASH_SHA256_NAMESPACE_FLAGGED_SIZE>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "RawBadEncodingFraudProof",
    into = "RawBadEncodingFraudProof"
)]
pub struct BadEncodingFraudProof {
    header_hash: Hash,
    block_height: Height,
    // ShareWithProof contains all shares from row or col.
    // Shares that did not pass verification in rsmt2d will be nil.
    // For non-nil shares MerkleProofs are computed.
    shares: Vec<ShareWithProof>,
    // Index represents the row/col index where ErrByzantineRow/ErrByzantineColl occurred.
    index: usize,
    // Axis represents the axis that verification failed on.
    axis: Axis,
}

impl FraudProof for BadEncodingFraudProof {
    const TYPE: &'static str = "badencoding";

    fn header_hash(&self) -> Hash {
        self.header_hash
    }

    fn height(&self) -> Height {
        self.block_height
    }

    fn validate(&self, header: &ExtendedHeader) -> Result<()> {
        if header.height() != self.height() {
            bail_validation!(
                "header height ({}) != fraud proof height ({})",
                header.height(),
                self.height()
            );
        }

        let merkle_row_roots = &header.dah.row_roots;
        let merkle_col_roots = &header.dah.column_roots;

        // NOTE: shouldn't ever happen as header should be validated before
        if merkle_row_roots.len() != merkle_col_roots.len() {
            bail_validation!(
                "dah rows len ({}) != dah columns len ({})",
                merkle_row_roots.len(),
                merkle_col_roots.len(),
            );
        }

        if self.index >= merkle_row_roots.len() {
            bail_validation!(
                "fraud proof index ({}) >= dah rows len ({})",
                self.index,
                merkle_row_roots.len()
            );
        }

        if self.shares.len() != merkle_row_roots.len() {
            bail_validation!(
                "fraud proof shares len ({}) != dah rows len ({})",
                self.shares.len(),
                merkle_row_roots.len()
            );
        }

        let root = match self.axis {
            Axis::Row => merkle_row_roots[self.index].clone(),
            Axis::Col => merkle_col_roots[self.index].clone(),
        };

        // verify if the root can be converted to a cid and back
        let mh = Multihash::wrap(MULTIHASH_SHA256_NAMESPACE_FLAGGED_CODE, &root.to_array())?;
        let cid = Cid::new_v1(MULTIHASH_NMT_CODEC_CODE, mh);
        let root = NamespacedHash::try_from(cid.hash().digest())?;

        // verify that Merkle proofs correspond to particular shares.
        for share in &self.shares {
            share
                .proof
                .verify_range(
                    &root,
                    &[share.leaf.share.as_ref()],
                    share.leaf.namespace.into(),
                )
                .map_err(Error::RangeProofError)?;
        }

        // TODO: Add leopard reed solomon decoding and encoding of shares
        //       and verify the nmt roots.

        Ok(())
    }
}

// TODO: this is not a Share but an Nmt Leaf, so it has it's namespace prepended.
//       It seems intentional in Celestia code, discuss with them what to do with this naming.
#[derive(Debug, Clone, PartialEq)]
struct ShareWithProof {
    leaf: NmtLeaf,
    proof: NamespaceProof,
}

#[derive(Debug, Clone, PartialEq)]
struct NmtLeaf {
    namespace: Namespace,
    share: Share,
}

impl NmtLeaf {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != appconsts::SHARE_SIZE + NS_SIZE {
            return Err(Error::InvalidNmtLeafSize(bytes.len()));
        }

        let (namespace, share) = bytes.split_at(NS_SIZE);

        Ok(Self {
            namespace: Namespace::from_raw(namespace)?,
            share: Share::from_raw(share)?,
        })
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut bytes = self.namespace.as_bytes().to_vec();
        bytes.extend_from_slice(self.share.as_ref());
        bytes
    }
}

impl TryFrom<RawShareWithProof> for ShareWithProof {
    type Error = Error;

    fn try_from(value: RawShareWithProof) -> Result<Self, Self::Error> {
        let leaf = NmtLeaf::new(value.data)?;

        let proof = value.proof.ok_or(Error::MissingProof)?;
        let proof = NamespaceProof::try_from(eds_proof_to_shrex(proof))?;

        if proof.is_of_absence() {
            return Err(Error::WrongProofType);
        }

        Ok(Self { leaf, proof })
    }
}

impl From<ShareWithProof> for RawShareWithProof {
    fn from(value: ShareWithProof) -> Self {
        let proof = RawShrexProof::from(value.proof);
        RawShareWithProof {
            data: value.leaf.to_vec(),
            proof: Some(shrex_proof_to_eds(proof)),
        }
    }
}

impl Protobuf<RawBadEncodingFraudProof> for BadEncodingFraudProof {}

impl TryFrom<RawBadEncodingFraudProof> for BadEncodingFraudProof {
    type Error = Error;

    fn try_from(value: RawBadEncodingFraudProof) -> Result<Self, Self::Error> {
        Ok(Self {
            header_hash: value.header_hash.try_into()?,
            block_height: value.height.try_into()?,
            shares: value
                .shares
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            index: value.index as usize,
            axis: value.axis.try_into()?,
        })
    }
}

impl From<BadEncodingFraudProof> for RawBadEncodingFraudProof {
    fn from(value: BadEncodingFraudProof) -> Self {
        RawBadEncodingFraudProof {
            header_hash: value.header_hash.into(),
            height: value.block_height.into(),
            shares: value.shares.into_iter().map(Into::into).collect(),
            index: value.index as u32,
            axis: value.axis as i32,
        }
    }
}

fn eds_proof_to_shrex(eds_proof: RawEdsProof) -> RawShrexProof {
    RawShrexProof {
        start: eds_proof.start,
        end: eds_proof.end,
        nodes: eds_proof.nodes,
        hashleaf: eds_proof.leaf_hash,
    }
}

fn shrex_proof_to_eds(shrex_proof: RawShrexProof) -> RawEdsProof {
    RawEdsProof {
        start: shrex_proof.start,
        end: shrex_proof.end,
        nodes: shrex_proof.nodes,
        leaf_hash: shrex_proof.hashleaf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fraud_proof::Proof;

    fn honest_befp() -> (BadEncodingFraudProof, ExtendedHeader) {
        let befp_json = include_str!("../test_data/fraud/honest_bad_encoding_fraud_proof.json");
        let eh_json = include_str!("../test_data/fraud/honest_bad_encoding_extended_header.json");
        let Proof::BadEncoding(proof) = serde_json::from_str(befp_json).unwrap();

        (proof, serde_json::from_str(eh_json).unwrap())
    }

    fn fake_befp() -> (BadEncodingFraudProof, ExtendedHeader) {
        let befp_json = include_str!("../test_data/fraud/fake_bad_encoding_fraud_proof.json");
        let eh_json = include_str!("../test_data/fraud/fake_bad_encoding_extended_header.json");
        let Proof::BadEncoding(proof) = serde_json::from_str(befp_json).unwrap();

        (proof, serde_json::from_str(eh_json).unwrap())
    }

    #[test]
    fn validate_honest_befp() {
        let (proof, eh) = honest_befp();
        proof.validate(&eh).unwrap();
    }

    #[test]
    fn validate_befp_wrong_height() {
        let (proof, mut eh) = honest_befp();
        eh.header.height = 999u32.into();

        proof.validate(&eh).unwrap_err();
    }

    #[test]
    fn validate_befp_wrong_roots_square() {
        let (proof, mut eh) = honest_befp();
        eh.dah.row_roots = vec![];

        proof.validate(&eh).unwrap_err();
    }

    #[test]
    fn validate_befp_wrong_index() {
        let (mut proof, eh) = honest_befp();
        proof.index = 999;

        proof.validate(&eh).unwrap_err();
    }

    #[test]
    fn validate_befp_wrong_shares() {
        let (mut proof, eh) = honest_befp();
        proof.shares = vec![];

        proof.validate(&eh).unwrap_err();
    }

    #[test]
    #[ignore = "TODO: we can't catch fake proofs without rebuilding the row using reedsolomon"]
    fn validate_fake_befp() {
        let (proof, eh) = fake_befp();
        proof.validate(&eh).unwrap_err();
    }
}
