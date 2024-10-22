//! Fraud proof related types and traits.
//!
//! A fraud proof is a proof of the detected malicious action done to the network.

use std::convert::Infallible;

use celestia_tendermint::block::Height;
use celestia_tendermint::Hash;
use celestia_tendermint_proto::Protobuf;
use serde::{Deserialize, Serialize, Serializer};

pub use crate::byzantine::BadEncodingFraudProof;
use crate::{Error, ExtendedHeader, Result};

/// A proof of the malicious actions done to the network.
pub trait FraudProof {
    /// Name of the proof type.
    const TYPE: &'static str;

    /// HeaderHash returns the block hash.
    fn header_hash(&self) -> Hash;

    /// Height returns the block height corresponding to the Proof.
    fn height(&self) -> Height;

    /// Checks the validity of the fraud proof.
    ///
    /// # Errors
    ///
    /// Returns an error if some conditions don't pass and thus fraud proof is not valid.
    fn validate(&self, header: &ExtendedHeader) -> Result<()>;
}

/// Raw representation of the generic fraud proof.
///
/// Consists of the name of the proof type and the protobuf serialized payload
/// holding the proof itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawFraudProof {
    proof_type: String,
    #[serde(with = "celestia_tendermint_proto::serializers::bytes::base64string")]
    data: Vec<u8>,
}

/// Aggregation of all the supported fraud proofs.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawFraudProof", into = "RawFraudProof")]
#[non_exhaustive]
pub enum Proof {
    /// A proof that a block producer incorrectly encoded [`ExtendedDataSquare`].
    ///
    /// [`ExtendedDataSquare`]: crate::eds::ExtendedDataSquare
    BadEncoding(BadEncodingFraudProof),
}

impl TryFrom<RawFraudProof> for Proof {
    type Error = Error;

    fn try_from(value: RawFraudProof) -> Result<Self, Self::Error> {
        match value.proof_type.as_str() {
            BadEncodingFraudProof::TYPE => {
                let befp = BadEncodingFraudProof::decode_vec(&value.data)?;
                Ok(Proof::BadEncoding(befp))
            }
            _ => Err(Error::UnsupportedFraudProofType(value.proof_type)),
        }
    }
}

impl From<&Proof> for RawFraudProof {
    fn from(value: &Proof) -> Self {
        match value {
            Proof::BadEncoding(befp) => {
                let encoded: Result<_, Infallible> = befp.encode_vec();
                RawFraudProof {
                    proof_type: BadEncodingFraudProof::TYPE.to_owned(),
                    data: encoded.unwrap(),
                }
            }
        }
    }
}

impl Serialize for Proof {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw: RawFraudProof = self.into();
        raw.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{corrupt_eds, generate_dummy_eds, ExtendedHeaderGenerator};

    use super::*;

    #[test]
    fn befp_serde() {
        let mut gen = ExtendedHeaderGenerator::new();
        let mut eds = generate_dummy_eds(8);
        let (_, proof) = corrupt_eds(&mut gen, &mut eds);

        let proof = Proof::BadEncoding(proof);

        let serialized = serde_json::to_string(&proof).unwrap();
        let deserialized: Proof = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized, proof);
    }
}
