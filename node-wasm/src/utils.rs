//! Various utilities for interacting with node from wasm.
use std::fmt::{self, Debug};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};
use wasm_bindgen::prelude::*;
use web_sys::{SharedWorker, SharedWorkerGlobalScope};

use lumina_node::network;

/// Supported Celestia networks.
#[wasm_bindgen]
#[derive(PartialEq, Eq, Clone, Copy, Serialize_repr, Deserialize_repr, Debug)]
#[repr(u8)]
pub enum Network {
    /// Celestia mainnet.
    Mainnet,
    /// Arabica testnet.
    Arabica,
    /// Mocha testnet.
    Mocha,
    /// Private local network.
    Private,
}

/// Set up a logging layer that direct logs to the browser's console.
#[wasm_bindgen(start)]
pub fn setup_logging() {
    console_error_panic_hook::set_once();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true) // Only partially supported across browsers, but we target only chrome now
        .with_timer(UtcTime::rfc_3339()) // std::time is not available in browsers
        .with_writer(MakeConsoleWriter) // write events to the console
        .with_filter(LevelFilter::INFO); // TODO: allow customizing the log level
    let perf_layer = performance_layer().with_details_from_fields(Pretty::default());

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(perf_layer)
        .init();
}

impl From<Network> for network::Network {
    fn from(network: Network) -> network::Network {
        match network {
            Network::Mainnet => network::Network::Mainnet,
            Network::Arabica => network::Network::Arabica,
            Network::Mocha => network::Network::Mocha,
            Network::Private => network::Network::Private,
        }
    }
}

impl From<network::Network> for Network {
    fn from(network: network::Network) -> Network {
        match network {
            network::Network::Mainnet => Network::Mainnet,
            network::Network::Arabica => Network::Arabica,
            network::Network::Mocha => Network::Mocha,
            network::Network::Private => Network::Private,
        }
    }
}

pub(crate) fn js_value_from_display<D: fmt::Display>(value: D) -> JsValue {
    JsValue::from(value.to_string())
}

pub(crate) trait JsContext<T> {
    fn js_context<C>(self, context: C) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static;

    fn with_js_context<F, C>(self, context_fn: F) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C;
}

impl<T, E> JsContext<T> for std::result::Result<T, E>
where
    E: std::error::Error,
{
    fn js_context<C>(self, context: C) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static,
    {
        self.map_err(|e| JsError::new(&format!("{context}: {e}")))
    }

    fn with_js_context<F, C>(self, context_fn: F) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map_err(|e| {
            let context = context_fn();
            JsError::new(&format!("{context}: {e}"))
        })
    }
}

pub(crate) trait WorkerSelf {
    type GlobalScope;

    fn worker_self() -> Self::GlobalScope;
}

impl WorkerSelf for SharedWorker {
    type GlobalScope = SharedWorkerGlobalScope;

    fn worker_self() -> Self::GlobalScope {
        JsValue::from(js_sys::global()).into()
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) enum JsResult<T, E>
where
    T: JsCast,
    //E: Serialize + DeserializeOwned,
{
    #[serde(with = "serde_wasm_bindgen::preserve")]
    Ok(T),
    Err(E),
}

// once try_trait_v2 is stabilised, this can go
impl<T, E> JsResult<T, E>
where
    T: JsCast,
    E: Serialize + DeserializeOwned,
{
    pub fn into_result(self) -> Result<T, E> {
        self.into()
    }
}

impl<T, E> From<Result<T, E>> for JsResult<T, E>
where
    T: JsCast,
    E: Serialize + DeserializeOwned,
{
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(v) => JsResult::Ok(v),
            Err(e) => JsResult::Err(e),
        }
    }
}

impl<T, E> From<JsResult<T, E>> for Result<T, E>
where
    T: JsCast,
    E: Serialize + DeserializeOwned,
{
    fn from(result: JsResult<T, E>) -> Self {
        match result {
            JsResult::Ok(v) => Ok(v),
            JsResult::Err(e) => Err(e),
        }
    }
}

impl<T, E> Debug for JsResult<T, E>
where
    T: JsCast,
    E: Serialize + DeserializeOwned,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsResult::Ok(_) => f.debug_tuple("JsResult::Ok"),
            JsResult::Err(_) => f.debug_tuple("JsResult::Err"),
        }
        .finish()
    }
}
