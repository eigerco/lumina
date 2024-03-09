use serde::{Deserialize, Serialize};
use tendermint_proto::v0_34::types::Blob as RawBlob;
use tendermint_proto::Protobuf;

mod commitment;

pub use self::commitment::Commitment;
use crate::consts::appconsts;
use crate::nmt::Namespace;
use crate::serializers::none_as_negative_one;
use crate::{bail_validation, Error, Result, Share};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubmitOptions {
    #[serde(with = "none_as_negative_one")]
    pub fee: Option<u64>,
    pub gas_limit: Option<u64>,
}

// NOTE: We don't use the `serde(try_from)` pattern for this type
// becase JSON representation needs to have `commitment` field but
// Protobuf definition doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub namespace: Namespace,
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    pub data: Vec<u8>,
    pub share_version: u8,
    pub commitment: Commitment,
    #[serde(default = "default_blob_index")]
    pub index: i64,
}

fn default_blob_index() -> i64 {
    -1
}

impl Blob {
    pub fn new(namespace: Namespace, data: Vec<u8>) -> Result<Blob> {
        let commitment =
            Commitment::from_blob(namespace, appconsts::SHARE_VERSION_ZERO, &data[..])?;

        Ok(Blob {
            namespace,
            data,
            share_version: appconsts::SHARE_VERSION_ZERO,
            commitment,
            index: -1
        })
    }

    pub fn validate(&self) -> Result<()> {
        let computed_commitment =
            Commitment::from_blob(self.namespace, self.share_version, &self.data)?;

        if self.commitment != computed_commitment {
            bail_validation!("blob commitment != localy computed commitment")
        }

        Ok(())
    }

    pub fn to_shares(&self) -> Result<Vec<Share>> {
        commitment::split_blob_to_shares(self.namespace, self.share_version, &self.data)
    }
}

impl Protobuf<RawBlob> for Blob {}

impl TryFrom<RawBlob> for Blob {
    type Error = Error;

    fn try_from(value: RawBlob) -> Result<Self, Self::Error> {
        let namespace = Namespace::new(value.namespace_version as u8, &value.namespace_id)?;
        let commitment =
            Commitment::from_blob(namespace, value.share_version as u8, &value.data[..])?;

        Ok(Blob {
            commitment,
            namespace,
            data: value.data,
            share_version: value.share_version as u8,
            index: -1
        })
    }
}

impl From<Blob> for RawBlob {
    fn from(value: Blob) -> RawBlob {
        RawBlob {
            namespace_id: value.namespace.id().to_vec(),
            namespace_version: value.namespace.version() as u32,
            data: value.data,
            share_version: value.share_version as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_blob() -> Blob {
        serde_json::from_str(
            r#"{
              "namespace": "AAAAAAAAAAAAAAAAAAAAAAAAAAAADCBNOWAP3dM=",
              "data": "8fIMqAB+kQo7+LLmHaDya8oH73hxem6lQWX1",
              "share_version": 0,
              "commitment": "D6YGsPWdxR8ju2OcOspnkgPG2abD30pSHxsFdiPqnVk="
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn create_from_raw() {
        let expected = sample_blob();
        let raw = RawBlob::from(expected.clone());
        let created = Blob::try_from(raw).unwrap();

        assert_eq!(created, expected);
    }

    #[test]
    fn validate_blob() {
        sample_blob().validate().unwrap();
    }

    #[test]
    fn validate_blob_commitment_mismatch() {
        let mut blob = sample_blob();
        blob.commitment.0.fill(7);

        blob.validate().unwrap_err();
    }
}
