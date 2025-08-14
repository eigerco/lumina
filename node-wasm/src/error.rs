//! Error type and utilities.

use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};
use wasm_bindgen::convert::IntoWasmAbi;
use wasm_bindgen::describe::WasmDescribe;
use wasm_bindgen::{JsCast, JsValue};

/// Alias for a `Result` with the error type [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// An error that can cross the WASM ABI border.
#[derive(Debug, Serialize, Deserialize)]
pub struct Error(#[serde(with = "serde_wasm_bindgen::preserve")] JsValue);

impl Error {
    /// Create a new `Error` with the specified message.
    pub fn new(msg: &str) -> Error {
        Error(js_sys::Error::new(msg).into())
    }

    /// Create a new `Error` from `JsValue` without losing any information.
    pub fn from_js_value<T>(value: T) -> Error
    where
        T: Into<JsValue>,
    {
        Error(value.into())
    }

    /// Create a new `Error` from any type that implements `Display`.
    ///
    /// This can be used on types that implement [`std::error::Error`] but
    /// some information is lost.
    pub fn from_display<T>(value: T) -> Error
    where
        T: Display,
    {
        Error::new(&value.to_string())
    }

    /// Add more context to the `Error`.
    pub fn context<C>(self, context: C) -> Error
    where
        C: Display,
    {
        let e = js_sys::Error::new(&context.to_string());
        e.set_cause(&self.0);
        Error(e.into())
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        let Some(error) = self.0.dyn_ref::<js_sys::Error>() else {
            return write!(f, "{:?}", self.0.as_string());
        };

        write!(f, "{} ({})", error.name(), error.message())?;

        let mut cause = error.cause();
        loop {
            if let Some(error) = cause.dyn_ref::<js_sys::Error>() {
                write!(f, "\n{} ({})", error.name(), error.message())?;
                cause = error.cause();
            } else {
                write!(f, "\n{:?}", cause.as_string())?;
                break;
            }
        }

        Ok(())
    }
}

// Similar to https://github.com/rustwasm/wasm-bindgen/blob/9b37613bbab0cd71f55dcc782d59dd00c1d4d200/src/describe.rs#L218-L222
//
// This is needed to impl IntoWasmAbi.
impl WasmDescribe for Error {
    fn describe() {
        JsValue::describe()
    }
}

// Similar to https://github.com/rustwasm/wasm-bindgen/blob/9b37613bbab0cd71f55dcc782d59dd00c1d4d200/src/convert/impls.rs#L468-L474
//
// This is needed to cross the ABI border.
impl IntoWasmAbi for Error {
    type Abi = <JsValue as IntoWasmAbi>::Abi;

    fn into_abi(self) -> Self::Abi {
        self.0.into_abi()
    }
}

impl From<Error> for JsValue {
    fn from(error: Error) -> JsValue {
        error.0
    }
}

/// Lossless convertion to `Error`.
///
/// Should be used for types that implement `Into<JsValue>`.
macro_rules! from_js_value {
    ($($t:ty,)*) => ($(
        impl From<$t> for Error {
            fn from(value: $t) -> Error {
                Error::from_js_value(value)
            }
        }
    )*)
}

/// Lossy convertion to `Error`.
///
/// Should be used for types that do not implement `Into<JsValue>`.
macro_rules! from_display {
    ($($t:ty,)*) => ($(
        impl From<$t> for Error {
            fn from(value: $t) -> Error {
                Error::from_display(value)
            }
        }
    )*)
}

from_js_value! {
    JsValue,
    serde_wasm_bindgen::Error,
    wasm_bindgen::JsError,
}

from_display! {
    blockstore::Error,
    tendermint::error::Error,
    libp2p::identity::ParseError,
    libp2p::multiaddr::Error,
    libp2p::identity::DecodingError,
    celestia_types::Error,
    lumina_node::node::NodeError,
    lumina_node::store::StoreError,
    crate::worker::WorkerError,
    tokio::sync::oneshot::error::RecvError,
    crate::key_registry::Error,
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(value: tokio::sync::mpsc::error::SendError<T>) -> Error {
        Error::from_display(value)
    }
}

impl<T> From<tokio::sync::broadcast::error::SendError<T>> for Error {
    fn from(value: tokio::sync::broadcast::error::SendError<T>) -> Error {
        Error::from_display(value)
    }
}

/// Utility to add more context to the [`Error`].
pub trait Context<T> {
    /// Adds more context to the [`Error`].
    fn context<C>(self, context: C) -> Result<T, Error>
    where
        C: Display;

    /// Adds more context to the [`Error`] that is evaluated lazily.
    fn with_context<F, C>(self, context_fn: F) -> Result<T, Error>
    where
        C: Display,
        F: FnOnce() -> C,
        Self: Sized,
    {
        self.context(context_fn())
    }
}

impl<T, E> Context<T> for Result<T, E>
where
    E: Into<Error>,
{
    fn context<C>(self, context: C) -> Result<T, Error>
    where
        C: Display,
    {
        self.map_err(|e| e.into().context(context))
    }
}

impl<T> Context<T> for Option<T> {
    fn context<C>(self, context: C) -> Result<T, Error>
    where
        C: Display,
    {
        self.ok_or_else(|| Error::new(&context.to_string()))
    }
}
