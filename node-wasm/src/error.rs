use std::fmt::Display;

use wasm_bindgen::convert::IntoWasmAbi;
use wasm_bindgen::describe::WasmDescribe;
use wasm_bindgen::JsValue;

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) struct Error(JsValue);

impl Error {
    /// Create a new `Error` with the specified message.
    pub(crate) fn new(msg: &str) -> Error {
        Error(js_sys::Error::new(msg).into())
    }

    /// Create a new `Error` from `JsValue` without lossing any information.
    pub(crate) fn from_js_value<T>(value: T) -> Error
    where
        T: Into<JsValue>,
    {
        Error(value.into())
    }

    /// Create a new `Error` from any type that implements `Display`.
    ///
    /// This can be used on types that implement [`std::error::Error`] but
    /// some information is lost.
    pub(crate) fn from_display<T>(value: T) -> Error
    where
        T: Display,
    {
        Error::new(&value.to_string())
    }

    /// Add more context to the `Error`.
    pub(crate) fn context<C>(self, context: C) -> Error
    where
        C: Display,
    {
        let e = js_sys::Error::new(&context.to_string());
        e.set_cause(&self.0);
        Error(e.into())
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
    celestia_tendermint::error::Error,
    libp2p::identity::ParseError,
    libp2p::multiaddr::Error,
    lumina_node::node::NodeError,
    lumina_node::store::StoreError,
}

pub(crate) trait Context<T> {
    fn context<C>(self, context: C) -> Result<T, Error>
    where
        C: Display;
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
