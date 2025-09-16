//! Types and client for the celestia grpc

use std::future::{Future, IntoFuture};

#[cfg(feature = "uniffi")]
use celestia_types::Hash;
use futures::future::{BoxFuture, FutureExt};
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

/// Context passed to each grpc request
#[derive(Debug, Default, Clone)]
pub(crate) struct Context {
    pub metadata: MetadataMap,
}

impl Context {
    pub(crate) fn append_metadata(&mut self, key: &str, val: &str) -> Result<(), MetadataError> {
        let key: MetadataKey<Ascii> = key.parse().map_err(|_| MetadataError::Key(key.into()))?;
        self.metadata.append(
            key,
            val.parse().map_err(|_| MetadataError::Value(val.into()))?,
        );

        Ok(())
    }

    pub(crate) fn append_metadata_bin(
        &mut self,
        key: &[u8],
        val: &[u8],
    ) -> Result<(), MetadataError> {
        let key = MetadataKey::from_bytes(key).map_err(|_| MetadataError::KeyBin(key.into()))?;
        self.metadata
            .append_bin(key, MetadataValue::from_bytes(val));

        Ok(())
    }

    pub(crate) fn append_metadata_map(&mut self, metadata: &MetadataMap) {
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

    pub(crate) fn extend(&mut self, other: &Context) {
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
/// # use celestia_types::AccAddress;
/// # async |client: GrpcClient, addr: &AccAddress| -> Result<()> {
/// let price = client.get_verified_balance(addr)
///     .metadata("x-token", "your secret token")?
///     .block_height(12345)
///     .await?;
/// # Ok(())
/// # };
/// ```
pub struct AsyncGrpcCall<Response> {
    call: CallFn<Response>,
    pub(crate) context: Context,
}

impl<Response> AsyncGrpcCall<Response> {
    /// Create a new grpc call out of the given function.
    pub(crate) fn new<F, Fut>(call_fn: F) -> Self
    where
        F: FnOnce(Context) -> Fut + 'static,
        Fut: Future<Output = Result<Response>> + Send + 'static,
    {
        Self {
            call: Box::new(|context| call_fn(context).boxed()),
            context: Context::default(),
        }
    }

    /// Extend the current context of the grpc call with provided context
    pub(crate) fn context(mut self, context: &Context) -> Self {
        self.context.extend(context);
        self
    }

    /// Append an ascii metadata to the grpc request.
    pub fn metadata(mut self, key: &str, val: &str) -> Result<Self, MetadataError> {
        self.context.append_metadata(key, val)?;
        Ok(self)
    }

    /// Append a binary metadata to the grpc request.
    pub fn metadata_bin(mut self, key: &[u8], val: &[u8]) -> Result<Self, MetadataError> {
        self.context.append_metadata_bin(key, val)?;
        Ok(self)
    }

    /// Append a metadata map to the grpc request.
    pub fn metadata_map(self, metadata: MetadataMap) -> Self {
        let context = Context { metadata };
        self.context(&context)
    }

    /// Performs the state queries at the specified height, unless node pruned it already.
    ///
    /// Appends `x-cosmos-block-height` metadata entry to the request.
    pub fn block_height(self, height: u64) -> Self {
        self.metadata("x-cosmos-block-height", &height.to_string())
            .expect("valid ascii metadata")
    }
}

impl<Response> IntoFuture for AsyncGrpcCall<Response> {
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
