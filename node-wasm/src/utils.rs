use std::fmt;

use lumina_node::network;
use serde_repr::{Deserialize_repr, Serialize_repr};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(PartialEq, Eq, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Network {
    Mainnet,
    Arabica,
    Mocha,
    Private,
}

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
