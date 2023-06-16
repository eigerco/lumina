use celestia_proto::tendermint::types as tmtypes;
use serde::{Deserialize, Deserializer, Serialize};

pub struct Blob {
    blob: tmtypes::Blob,
    commitment: Vec<u8>,
}

/// Helper type for serialization
///
/// Ref: https://github.com/celestiaorg/celestia-node/blob/6654cdf4994dbd381efd0d6a29688c731177c855/blob/blob.go#L134-L164
#[derive(Serialize, Deserialize)]
struct BlobJson {
    namespace: Vec<u8>, // TODO: namespace ID
    #[serde(with = "crate::serde_base64")]
    data: Vec<u8>,
    share_version: u32,
    commitment: Vec<u8>, // TODO: Commitment
}

impl Serialize for Blob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let blob_json = BlobJson {
            namespace: [
                &[self.blob.namespace_version as u8],
                &self.blob.namespace_id[..],
            ]
            .concat(),
            data: self.blob.data.clone(),
            share_version: self.blob.share_version,
            commitment: self.commitment.clone(),
        };

        blob_json.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let blob_json = BlobJson::deserialize(deserializer)?;

        let blob = Blob {
            blob: tmtypes::Blob {
                namespace_id: blob_json.namespace[1..].to_vec(),
                namespace_version: blob_json.namespace[0] as u32,
                data: blob_json.data,
                share_version: blob_json.share_version,
            },
            commitment: blob_json.commitment,
        };

        Ok(blob)
    }
}
