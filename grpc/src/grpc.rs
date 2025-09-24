//! Types and client for the celestia grpc

use std::future::{Future, IntoFuture};
use std::{any, fmt};

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

type RequestFuture<Response, Error> = BoxFuture<'static, Result<Response, Error>>;
type CallFn<Response, Error> = Box<dyn FnOnce(Context) -> RequestFuture<Response, Error>>;

/// Context passed to each grpc request
///
/// This type is exposed only for internal use in `celestia-client`.
/// It is not considered a public API and thus have no SemVer guarantees.
#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// Metadata attached to each grpc request.
    pub metadata: MetadataMap,
}

impl Context {
    /// Appends an ascii metadata entry to the map. Ignores duplicate values.
    pub(crate) fn append_metadata(&mut self, key: &str, val: &str) -> Result<(), MetadataError> {
        let key: MetadataKey<Ascii> = key.parse().map_err(|_| MetadataError::Key(key.into()))?;
        let value = val.parse().map_err(|_| MetadataError::Value(key.into()))?;

        if !self
            .metadata
            .get_all(&key)
            .into_iter()
            .any(|val| val == value)
        {
            self.metadata.append(key, value);
        }

        Ok(())
    }

    /// Appends a binary metadata entry to the map. Ignores duplicate values.
    ///
    /// For binary methadata, key must end with `-bin`.
    pub(crate) fn append_metadata_bin(
        &mut self,
        key: &str,
        val: &[u8],
    ) -> Result<(), MetadataError> {
        let key = MetadataKey::from_bytes(key.as_bytes())
            .map_err(|_| MetadataError::KeyBin(key.into()))?;
        let value = MetadataValue::from_bytes(val);

        if !self
            .metadata
            .get_all_bin(&key)
            .into_iter()
            .any(|val| val == value)
        {
            self.metadata.append_bin(key, value);
        }

        Ok(())
    }

    /// Appends whole metadata map to the current metadata map.
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

    /// Merges the other context into self.
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
/// let balance = client.get_verified_balance(addr)
///     .metadata("x-token", "your secret token")?
///     .block_height(12345)
///     .await?;
/// # Ok(())
/// # };
/// ```
pub struct AsyncGrpcCall<Response, Error = crate::Error> {
    call: CallFn<Response, Error>,
    pub(crate) context: Context,
}

impl<Response, Error> AsyncGrpcCall<Response, Error> {
    /// Create a new grpc call out of the given function.
    ///
    /// This method is exposed only for internal use in `celestia-client`.
    /// It is not considered a public API and thus have no SemVer guarantees.
    #[doc(hidden)]
    pub fn new<F, Fut>(call_fn: F) -> Self
    where
        F: FnOnce(Context) -> Fut + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        Self {
            call: Box::new(|context| call_fn(context).boxed()),
            context: Context::default(),
        }
    }

    /// Extend the current context of the grpc call with provided context
    ///
    /// This method is exposed only for internal use in `celestia-client`.
    /// It is not considered a public API and thus have no SemVer guarantees.
    #[doc(hidden)]
    pub fn context(mut self, context: &Context) -> Self {
        self.context.extend(context);
        self
    }

    /// Append an ascii metadata to the grpc request.
    pub fn metadata(mut self, key: &str, val: &str) -> Result<Self, MetadataError> {
        self.context.append_metadata(key, val)?;
        Ok(self)
    }

    /// Append a binary metadata to the grpc request.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    pub fn metadata_bin(mut self, key: &str, val: &[u8]) -> Result<Self, MetadataError> {
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

impl<Response, Error> IntoFuture for AsyncGrpcCall<Response, Error> {
    type Output = Result<Response, Error>;
    type IntoFuture = RequestFuture<Response, Error>;

    fn into_future(self) -> Self::IntoFuture {
        (self.call)(self.context)
    }
}

impl<Response> fmt::Debug for AsyncGrpcCall<Response> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("AsyncGrpcCall<{}>", any::type_name::<Response>()))
            .field("context", &self.context)
            .finish()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_appending_metadata() {
        let mut context = Context::default();

        // ascii
        context.append_metadata("foo", "bar").unwrap();
        context.append_metadata("foo", "bar").unwrap();

        assert_eq!(context.metadata.get_all("foo").into_iter().count(), 1);

        context.append_metadata("foo", "bar2").unwrap();

        assert_eq!(context.metadata.get_all("foo").into_iter().count(), 2);

        // binary
        context.append_metadata_bin("foo-bin", b"bar").unwrap();
        context.append_metadata_bin("foo-bin", b"bar").unwrap();

        assert_eq!(
            context.metadata.get_all_bin("foo-bin").into_iter().count(),
            1
        );

        context.append_metadata_bin("foo-bin", b"bar2").unwrap();

        assert_eq!(
            context.metadata.get_all_bin("foo-bin").into_iter().count(),
            2
        );
    }
}
