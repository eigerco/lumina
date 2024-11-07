use cosmrs::Tx;

use celestia_proto::cosmos::base::abci::v1beta1::TxResponse as RawTxResponse;
use celestia_proto::cosmos::tx::v1beta1::{BroadcastTxResponse, GetTxResponse as RawGetTxResponse};

use crate::types::FromGrpcResponse;
use crate::Error;

/// Response to a tx query
pub struct TxResponse {
    /// The block height
    pub height: i64,

    /// The transaction hash.
    pub txhash: String,

    /// Namespace for the Code
    pub codespace: String,

    /// Response code.
    pub code: u32,

    /// Result bytes, if any.
    pub data: String,

    /// The output of the application's logger (raw string). May be
    /// non-deterministic.
    pub raw_log: String,

    // The output of the application's logger (typed). May be non-deterministic.
    //#[serde(with = "crate::serializers::null_default")]
    //pub logs: ::prost::alloc::vec::Vec<AbciMessageLog>,
    /// Additional information. May be non-deterministic.
    pub info: String,

    /// Amount of gas requested for transaction.
    pub gas_wanted: i64,

    /// Amount of gas consumed by transaction.
    pub gas_used: i64,

    // The request transaction bytes.
    //#[serde(with = "crate::serializers::option_any")]
    //pub tx: ::core::option::Option<::pbjson_types::Any>,
    /// Time of the previous block. For heights > 1, it's the weighted median of
    /// the timestamps of the valid votes in the block.LastCommit. For height == 1,
    /// it's genesis time.
    pub timestamp: String,
    // Events defines all the events emitted by processing a transaction. Note,
    // these events include those emitted by processing all the messages and those
    // emitted from the ante. Whereas Logs contains the events, with
    // additional metadata, emitted only by processing the messages.
    //
    // Since: cosmos-sdk 0.42.11, 0.44.5, 0.45
    //#[serde(with = "crate::serializers::null_default")]
    //pub events: ::prost::alloc::vec::Vec< ::celestia_tendermint_proto::v0_34::abci::Event, >,
}

/// Response to GetTx
pub struct GetTxResponse {
    /// Response Transaction
    pub tx: Tx,

    /// TxResponse to a Query
    pub tx_response: TxResponse,
}

impl TryFrom<RawTxResponse> for TxResponse {
    type Error = Error;

    fn try_from(response: RawTxResponse) -> Result<TxResponse, Self::Error> {
        // TODO: add missing fields with conversion
        Ok(TxResponse {
            height: response.height,
            txhash: response.txhash,
            codespace: response.codespace,
            code: response.code,
            data: response.data,
            raw_log: response.raw_log,
            //logs: response.logs
            info: response.info,
            gas_wanted: response.gas_wanted,
            gas_used: response.gas_used,
            //tx: response.tx
            timestamp: response.timestamp,
            //events: response.events
        })
    }
}

impl FromGrpcResponse<TxResponse> for BroadcastTxResponse {
    fn try_from_response(self) -> Result<TxResponse, Error> {
        self.tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()
    }
}

impl FromGrpcResponse<GetTxResponse> for RawGetTxResponse {
    fn try_from_response(self) -> Result<GetTxResponse, Error> {
        let tx_response = self
            .tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?;
        let tx = self.tx.ok_or(Error::FailedToParseResponse)?;
        let cosmos_tx = Tx {
            body: tx.body.ok_or(Error::FailedToParseResponse)?.try_into()?,
            auth_info: tx
                .auth_info
                .ok_or(Error::FailedToParseResponse)?
                .try_into()?,
            signatures: tx.signatures,
        };

        Ok(GetTxResponse {
            tx: cosmos_tx,
            tx_response,
        })
    }
}
