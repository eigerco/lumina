use celestia_proto::cosmos::base::abci::v1beta1::GasInfo;
use celestia_types::hash::Hash;

use celestia_proto::cosmos::tx::v1beta1::{
    BroadcastTxRequest, BroadcastTxResponse, GetTxRequest as RawGetTxRequest,
    GetTxResponse as RawGetTxResponse, SimulateRequest, SimulateResponse,
};
use celestia_types::state::{Tx, TxResponse};

use crate::grpc::{FromGrpcResponse, IntoGrpcParam};
use crate::{Error, Result};

pub use celestia_proto::cosmos::tx::v1beta1::BroadcastMode;

/// Response to GetTx
#[derive(Debug)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GetTxResponse {
    /// Response Transaction
    pub tx: Tx,

    /// TxResponse to a Query
    pub tx_response: TxResponse,
}

impl IntoGrpcParam<BroadcastTxRequest> for (Vec<u8>, BroadcastMode) {
    fn into_parameter(self) -> BroadcastTxRequest {
        let (tx_bytes, mode) = self;

        BroadcastTxRequest {
            tx_bytes,
            mode: mode.into(),
        }
    }
}

impl FromGrpcResponse<TxResponse> for BroadcastTxResponse {
    fn try_from_response(self) -> Result<TxResponse> {
        Ok(self
            .tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?)
    }
}

impl IntoGrpcParam<RawGetTxRequest> for Hash {
    fn into_parameter(self) -> RawGetTxRequest {
        RawGetTxRequest {
            hash: self.to_string(),
        }
    }
}

impl FromGrpcResponse<GetTxResponse> for RawGetTxResponse {
    fn try_from_response(self) -> Result<GetTxResponse> {
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

impl IntoGrpcParam<SimulateRequest> for Vec<u8> {
    fn into_parameter(self) -> SimulateRequest {
        SimulateRequest {
            tx_bytes: self,
            ..SimulateRequest::default()
        }
    }
}

impl FromGrpcResponse<GasInfo> for SimulateResponse {
    fn try_from_response(self) -> Result<GasInfo> {
        self.gas_info.ok_or(Error::FailedToParseResponse)
    }
}
