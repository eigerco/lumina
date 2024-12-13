use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::blob::{Blob, Commitment};
use crate::nmt::Namespace;
use crate::state::Address;
use crate::{Error, Result};

pub use celestia_proto::celestia::blob::v1::MsgPayForBlobs as RawMsgPayForBlobs;

/// MsgPayForBlobs pays for the inclusion of a blob in the block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgPayForBlobs {
    /// signer is the bech32 encoded signer address
    pub signer: Address,
    /// namespaces is a list of namespaces that the blobs are associated with.
    pub namespaces: Vec<Namespace>,
    /// sizes of the associated blobs
    pub blob_sizes: Vec<u32>,
    /// share_commitments is a list of share commitments (one per blob).
    pub share_commitments: Vec<Commitment>,
    /// share_versions are the versions of the share format that the blobs
    /// associated with this message should use when included in a block. The
    /// share_versions specified must match the share_versions used to generate the
    /// share_commitment in this message.
    pub share_versions: Vec<u32>,
}

impl MsgPayForBlobs {
    /// Create a pay for blobs message for the provided Blobs and signer
    pub fn new(blobs: &[Blob], signer: Address) -> Result<Self> {
        let blob_count = blobs.len();
        if blob_count == 0 {
            return Err(Error::EmptyBlobList);
        }
        let mut blob_sizes = Vec::with_capacity(blob_count);
        let mut namespaces = Vec::with_capacity(blob_count);
        let mut share_commitments = Vec::with_capacity(blob_count);
        let mut share_versions = Vec::with_capacity(blob_count);
        for blob in blobs {
            blob_sizes.push(u32::try_from(blob.data.len()).map_err(|_| Error::BlobTooLarge)?);
            namespaces.push(blob.namespace);
            share_commitments.push(blob.commitment);
            share_versions.push(u32::from(blob.share_version));
        }

        Ok(Self {
            signer,
            namespaces,
            blob_sizes,
            share_commitments,
            share_versions,
        })
    }
}

impl From<MsgPayForBlobs> for RawMsgPayForBlobs {
    fn from(msg: MsgPayForBlobs) -> Self {
        let namespaces = msg
            .namespaces
            .into_iter()
            .map(|n| n.as_bytes().to_vec())
            .collect();
        let share_commitments = msg
            .share_commitments
            .into_iter()
            .map(|c| c.0.to_vec())
            .collect();

        RawMsgPayForBlobs {
            signer: msg.signer.to_string(),
            namespaces,
            blob_sizes: msg.blob_sizes,
            share_commitments,
            share_versions: msg.share_versions,
        }
    }
}

impl TryFrom<RawMsgPayForBlobs> for MsgPayForBlobs {
    type Error = Error;

    fn try_from(msg: RawMsgPayForBlobs) -> Result<MsgPayForBlobs, Self::Error> {
        let namespaces = msg
            .namespaces
            .into_iter()
            .map(|n| Namespace::from_raw(&n))
            .collect::<Result<_, Error>>()?;
        let share_commitments = msg
            .share_commitments
            .into_iter()
            .map(|c| {
                Ok(Commitment(
                    c.try_into().map_err(|_| Error::InvalidComittmentLength)?,
                ))
            })
            .collect::<Result<_, Error>>()?;

        Ok(MsgPayForBlobs {
            signer: msg.signer.parse()?,
            namespaces,
            blob_sizes: msg.blob_sizes,
            share_commitments,
            share_versions: msg.share_versions,
        })
    }
}

impl Protobuf<RawMsgPayForBlobs> for MsgPayForBlobs {}
