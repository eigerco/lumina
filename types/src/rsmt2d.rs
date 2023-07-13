use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Axis {
    Row = 0,
    Col,
}

impl TryFrom<i32> for Axis {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            x if x == Axis::Row as i32 => Ok(Axis::Row),
            x if x == Axis::Col as i32 => Ok(Axis::Col),
            x => Err(Error::InvalidAxis(x)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedDataSquare {
    #[serde(with = "tendermint_proto::serializers::bytes::vec_base64string")]
    pub data_square: Vec<Vec<u8>>,
    pub codec: String,
}
