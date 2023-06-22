use serde::{Deserialize, Serialize};
use tendermint_proto::v0_34::types::Blob as RawBlob;
use tendermint_proto::Protobuf;

use crate::nmt::Namespace;
use crate::Error;

// NOTE: We don't use the `serde(try_from)` pattern for this type
// becase JSON representation needs to have `commitment` field but
// Protobuf definition doesn't.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blob {
    pub namespace: Namespace,
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    pub data: Vec<u8>,
    pub share_version: u8,
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    pub commitment: Vec<u8>,
}

impl Protobuf<RawBlob> for Blob {}

impl TryFrom<RawBlob> for Blob {
    type Error = Error;

    fn try_from(value: RawBlob) -> Result<Self, Self::Error> {
        Ok(Blob {
            namespace: Namespace::new(value.namespace_version as u8, &value.namespace_id)?,
            data: value.data,
            share_version: value.share_version as u8,
            commitment: Vec::new(),
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
