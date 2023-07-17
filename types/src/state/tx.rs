use celestia_proto::cosmos::base::abci::v1beta1::TxResponse as RawTxResponse;
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

pub type TxResponse = RawTxResponse;
