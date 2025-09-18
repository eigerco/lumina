//! Types and client for the celestia grpc

#[cfg(feature = "uniffi")]
use celestia_types::Hash;

use crate::Result;

// cosmos.auth
mod auth;
// cosmos.bank
mod bank;
// celestia.core.gas_estimation
mod gas_estimation;
// cosmos.base.node
mod node;
// cosmos.base.tendermint
mod tendermint;
// cosmos.staking
mod staking;
// celestia.core.tx
mod celestia_tx;
// celestia.blob
mod blob;
// cosmos.tx
mod cosmos_tx;

pub use crate::grpc::celestia_tx::{TxStatus, TxStatusResponse};
pub use crate::grpc::cosmos_tx::{BroadcastMode, GetTxResponse};
pub use crate::grpc::gas_estimation::{GasEstimate, TxPriority};
pub use crate::grpc::node::ConfigResponse;
pub use celestia_proto::cosmos::base::abci::v1beta1::GasInfo;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use crate::grpc::cosmos_tx::JsBroadcastMode;

#[cfg(feature = "uniffi")]
uniffi::use_remote_type!(celestia_types::Hash);

pub(crate) trait FromGrpcResponse<T> {
    fn try_from_response(self) -> Result<T>;
}

pub(crate) trait IntoGrpcParam<T> {
    fn into_parameter(self) -> T;
}

macro_rules! make_empty_params {
    ($request_type:ident) => {
        impl crate::grpc::IntoGrpcParam<$request_type> for () {
            fn into_parameter(self) -> $request_type {
                $request_type {}
            }
        }
    };
}

pub(crate) use make_empty_params;
