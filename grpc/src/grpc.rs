//! Types and client for the celestia grpc

use std::future::IntoFuture;

#[cfg(feature = "uniffi")]
use celestia_types::Hash;
use futures::future::BoxFuture;
use tonic::metadata::{Ascii, KeyAndValueRef, MetadataKey, MetadataMap, MetadataValue};

use crate::error::MetadataError;
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

type RequestFuture<Response> = BoxFuture<'static, Result<Response>>;
type CallFn<Response> = Box<dyn FnOnce(Context) -> RequestFuture<Response>>;

#[derive(Debug, Default, Clone)]
pub struct Context {
    pub metadata: MetadataMap,
}

impl Context {
    pub fn append_metadata(&mut self, key: &str, val: &str) -> Result<(), MetadataError> {
        let key: MetadataKey<Ascii> = key.parse().map_err(|_| MetadataError::Key(key.into()))?;
        self.metadata.append(
            key,
            val.parse().map_err(|_| MetadataError::Value(val.into()))?,
        );

        Ok(())
    }

    pub fn append_metadata_bin(&mut self, key: &[u8], val: &[u8]) -> Result<(), MetadataError> {
        let key = MetadataKey::from_bytes(key).map_err(|_| MetadataError::KeyBin(key.into()))?;
        self.metadata
            .append_bin(key, MetadataValue::from_bytes(val));

        Ok(())
    }

    pub fn append_metadata_map(&mut self, metadata: &MetadataMap) {
        for key_and_value in metadata.iter() {
            match key_and_value {
                KeyAndValueRef::Ascii(key, val) => {
                    self.metadata.append(key, val.clone());
                }
                KeyAndValueRef::Binary(key, val) => {
                    self.metadata.append_bin(key, val.clone());
                }
            }
        }
    }

    pub fn extend(&mut self, other: &Context) {
        self.append_metadata_map(&other.metadata);
    }
}

/// A call of the grpc method.
///
/// Allows setting additional context for request before awaiting
/// the call.
///
/// ```
/// # use celestia_grpc::{Result, GrpcClient};
/// # use celestia_grpc::grpc::TxPriority;
/// # async |client: GrpcClient| -> Result<()> {
/// let price = client.estimate_gas_price(TxPriority::Low)
///     .metadata("x-token", "your secret token")?
///     .await?;
/// # Ok(())
/// # };
/// ```
pub struct GrpcCall<Response> {
    call: CallFn<Response>,
    pub context: Context,
}

impl<Response> GrpcCall<Response> {
    /// Create a new grpc call out of the given function.
    pub fn new<F>(call: F, context: Context) -> Self
    where
        F: FnOnce(Context) -> RequestFuture<Response> + 'static,
    {
        Self {
            call: Box::new(call),
            context,
        }
    }

    pub fn context(mut self, context: &Context) -> Self {
        self.context.extend(context);
        self
    }

    pub fn metadata(mut self, key: &str, val: &str) -> Result<Self, MetadataError> {
        self.context.append_metadata(key, val)?;
        Ok(self)
    }

    pub fn metadata_bin(mut self, key: &[u8], val: &[u8]) -> Result<Self, MetadataError> {
        self.context.append_metadata_bin(key, val)?;
        Ok(self)
    }

    pub fn metadata_map(self, metadata: MetadataMap) -> Self {
        let context = Context { metadata };
        self.context(&context)
    }
}

impl<Response> IntoFuture for GrpcCall<Response> {
    type Output = Result<Response>;
    type IntoFuture = RequestFuture<Response>;

    fn into_future(self) -> Self::IntoFuture {
        (self.call)(self.context)
    }
}

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
