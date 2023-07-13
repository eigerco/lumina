use std::convert::Infallible;

use serde::{Deserialize, Serialize, Serializer};
use tendermint::block::Height;
use tendermint::Hash;
use tendermint_proto::Protobuf;

use crate::{byzantine::BadEncodingFraudProof, Error, ExtendedHeader, Result};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProofType<'a>(pub &'a str);

pub trait FraudProof {
    const TYPE: ProofType<'static>;

    // HeaderHash returns the block hash.
    fn header_hash(&self) -> Hash;

    // Height returns the block height corresponding to the Proof.
    fn height(&self) -> Height;

    // Checks the validity of the fraud proof.
    //
    // # Errors
    // Returns an error if some conditions don't pass and thus fraud proof is not valid.
    fn validate(&self, header: &ExtendedHeader) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawFraudProof {
    proof_type: String,
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawFraudProof", into = "RawFraudProof")]
#[non_exhaustive]
pub enum Proof {
    BadEncoding(BadEncodingFraudProof),
}

impl TryFrom<RawFraudProof> for Proof {
    type Error = Error;

    fn try_from(value: RawFraudProof) -> Result<Self, Self::Error> {
        match ProofType(&value.proof_type) {
            BadEncodingFraudProof::TYPE => {
                let befp = BadEncodingFraudProof::decode_vec(&value.data).unwrap();
                Ok(Proof::BadEncoding(befp))
            }
            _ => todo!(),
        }
    }
}

impl From<Proof> for RawFraudProof {
    fn from(value: Proof) -> Self {
        match value {
            Proof::BadEncoding(befp) => {
                let encoded: Result<_, Infallible> = befp.encode_vec();
                RawFraudProof {
                    proof_type: BadEncodingFraudProof::TYPE.0.to_owned(),
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
        let raw: RawFraudProof = self.clone().try_into().map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}
