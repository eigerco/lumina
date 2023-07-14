use serde::{Deserialize, Serialize};

/// Raw TX data
///
/// NOTE: Tx has no types at this level
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawTx {
    #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
    data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse;
