use celestia_proto::tendermint_celestia_mods::types::Data as RawData;
use serde::{Deserialize, Serialize};
use tendermint_proto::Protobuf;

use crate::Error;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "RawData", into = "RawData")]
pub struct Data {
    pub txs: Vec<Vec<u8>>,
    pub square_size: u64,
    pub hash: Vec<u8>,
}

impl Protobuf<RawData> for Data {}

impl TryFrom<RawData> for Data {
    type Error = Error;

    fn try_from(value: RawData) -> Result<Self, Self::Error> {
        Ok(Data {
            txs: value.txs,
            square_size: value.square_size,
            hash: value.hash,
        })
    }
}

impl From<Data> for RawData {
    fn from(value: Data) -> RawData {
        RawData {
            txs: value.txs,
            square_size: value.square_size,
            hash: value.hash,
        }
    }
}
