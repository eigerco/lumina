//! Various utilities for interacting with node from wasm.
use std::fmt::{self, Debug};

use js_sys::JsString;
use lumina_node::network;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_wasm_bindgen::to_value;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};
use wasm_bindgen::prelude::*;
use web_sys::{Crypto, DedicatedWorkerGlobalScope, SharedWorker, SharedWorkerGlobalScope, Worker};

use crate::error::{Context, Error, Result};

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

pub(crate) fn to_jsvalue_or_undefined<T: Serialize>(value: &T) -> JsValue {
    to_value(value).unwrap_or(JsValue::UNDEFINED)
}

pub(crate) trait JsValueToJsError<T> {
    fn to_error<C>(self, context_fn: C) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static;
}

impl<T> JsValueToJsError<T> for std::result::Result<T, JsValue> {
    fn to_error<C>(self, context: C) -> Result<T, JsError>
    where
        C: fmt::Display + Send + Sync + 'static,
    {
        self.map_err(|e| {
            let error_str = match e.dyn_ref::<JsString>() {
                Some(s) => format!("{context}: {s}"),
                None => format!("{context}"),
            };
            JsError::new(&error_str)
        })
    }
}

pub(crate) trait WorkerSelf {
    type GlobalScope;

    fn worker_self() -> Self::GlobalScope;
    fn is_worker_type() -> bool;
}

impl WorkerSelf for SharedWorker {
    type GlobalScope = SharedWorkerGlobalScope;

    fn worker_self() -> Self::GlobalScope {
        JsValue::from(js_sys::global()).into()
    }

    fn is_worker_type() -> bool {
        js_sys::global().has_type::<Self::GlobalScope>()
    }
}

impl WorkerSelf for Worker {
    type GlobalScope = DedicatedWorkerGlobalScope;

    fn worker_self() -> Self::GlobalScope {
        JsValue::from(js_sys::global()).into()
    }

    fn is_worker_type() -> bool {
        js_sys::global().has_type::<Self::GlobalScope>()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum JsResult<T, E>
where
    T: JsCast + Debug,
    E: Debug,
{
    #[serde(with = "serde_wasm_bindgen::preserve")]
    Ok(T),
    Err(E),
}

// once try_trait_v2 is stabilised, this can go
impl<T, E> JsResult<T, E>
where
    T: JsCast + Debug,
    E: Serialize + DeserializeOwned + Debug,
{
    pub fn into_result(self) -> Result<T, E> {
        self.into()
    }
}

impl<T, E> From<Result<T, E>> for JsResult<T, E>
where
    T: JsCast + Debug,
    E: Serialize + DeserializeOwned + Debug,
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
    T: JsCast + Debug,
    E: Serialize + DeserializeOwned + Debug,
{
    fn from(result: JsResult<T, E>) -> Self {
        match result {
            JsResult::Ok(v) => Ok(v),
            JsResult::Err(e) => Err(e),
        }
    }
}

const CHROME_USER_AGENT_DETECTION_STR: &str = "Chrome/";

// currently there's issue with SharedWorkers on Chrome, where restarting lumina's worker
// causes all network connections to fail. Until that's resolved detect chrome and apply
// a workaround.
pub(crate) fn is_chrome() -> Result<bool, Error> {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))
        .context("failed to get `navigator` from global object")?
        .dyn_into::<web_sys::Navigator>()
        .context("`navigator` is not instanceof `Navigator`")?
        .user_agent()
        .context("could not get UserAgent from Navigator")
        .map(|user_agent| user_agent.contains(CHROME_USER_AGENT_DETECTION_STR))
}

pub(crate) fn get_crypto() -> Result<Crypto, Error> {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crypto"))
        .context("failed to get `crypto` from global object")?
        .dyn_into::<web_sys::Crypto>()
        .ok()
        .context("`crypto` is not `Crypto` type")
}
