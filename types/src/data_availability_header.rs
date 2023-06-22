use celestia_proto::celestia::da::DataAvailabilityHeader as RawDataAvailabilityHeader;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tendermint::merkle::{simple_hash_from_byte_vectors, Hash};
use tendermint_proto::Protobuf;

use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    try_from = "RawDataAvailabilityHeader",
    into = "RawDataAvailabilityHeader"
)]
pub struct DataAvailabilityHeader {
    pub row_roots: Vec<Vec<u8>>,
    pub column_roots: Vec<Vec<u8>>,
    pub hash: Hash,
}

impl Protobuf<RawDataAvailabilityHeader> for DataAvailabilityHeader {}

impl TryFrom<RawDataAvailabilityHeader> for DataAvailabilityHeader {
    type Error = Error;

    fn try_from(value: RawDataAvailabilityHeader) -> Result<Self, Self::Error> {
        let all_roots: Vec<_> = value
            .row_roots
            .iter()
            .chain(value.column_roots.iter())
            .collect();
        let hash = simple_hash_from_byte_vectors::<Sha256>(&all_roots);

        Ok(DataAvailabilityHeader {
            row_roots: value.row_roots,
            column_roots: value.column_roots,
            hash,
        })
    }
}

impl From<DataAvailabilityHeader> for RawDataAvailabilityHeader {
    fn from(value: DataAvailabilityHeader) -> RawDataAvailabilityHeader {
        RawDataAvailabilityHeader {
            row_roots: value.row_roots,
            column_roots: value.column_roots,
        }
    }
}
