//! Various utilities for interacting with node from wasm.

use std::fmt;
use std::marker::PhantomData;

use lumina_node::network;
use serde_repr::{Deserialize_repr, Serialize_repr};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};
use wasm_bindgen::prelude::*;

use crate::node::WasmNodeConfig;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use serde_wasm_bindgen::to_value;
use tokio::sync::mpsc;
use wasm_bindgen_futures::spawn_local;
use web_sys::MessageEvent;
use web_sys::MessagePort;
use web_sys::SharedWorker;
use web_sys::SharedWorkerGlobalScope;

pub struct BChannel<IN, OUT> {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    channel: MessagePort,
    msg_rx: mpsc::Receiver<OUT>,
    send_type: PhantomData<IN>,
}

impl<IN, OUT> BChannel<IN, OUT>
where
    IN: Serialize,
    OUT: DeserializeOwned + 'static,
{
    pub fn new(channel: MessagePort) -> Self {
        let (tx, rx) = mpsc::channel(64);

        let near_tx = tx.clone();
        let f = move |ev: MessageEvent| {
            let local_tx = near_tx.clone();
            spawn_local(async move {
                let message_data = ev.data();

                let data = from_value(message_data).unwrap();

                local_tx.send(data).await.expect("send err");
            })
        };

        let onmessage = Closure::new(f);

        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        channel.start();
        Self {
            _onmessage: onmessage,
            channel,
            msg_rx: rx,
            send_type: PhantomData,
        }
    }

    pub fn send(&self, msg: IN) {
        let v = to_value(&msg).unwrap();
        self.channel.post_message(&v).expect("err post");
    }

    pub async fn recv(&mut self) -> Option<OUT> {
        self.msg_rx.recv().await
    }
}

// TODO: cleanup JS objects on drop
// impl Drop

/*
#[derive(Serialize, Deserialize)]
enum BroadcastMessage {
    Foo
}

fn create_channel() {

    let broadcast_channel = BChannel::new("lumina");

    broadcast_channel.send(BroadcastMessage::Foo);
}
*/

/*
#[derive(Serialize, Deserialize, Debug)]
pub enum NodeCommand {
    //ConnectDriver(u64),
    Start(WasmNodeConfig),
    GetLocalPeerId,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NodeResponse {
    LocalPeerId(String),
}
*/

/*
#[wasm_bindgen]
struct BSharedWorker {
    worker: SharedWorker,
    channel: BChannel<NodeCommand, NodeResponse>,
}

#[wasm_bindgen]
impl BSharedWorker {
    pub fn new() -> Self {
        let mut opts = WorkerOptions::new();
        opts.type_(WorkerType::Module);
        let worker = SharedWorker::new_with_worker_options("/js/worker.js", &opts)
            .expect("could not worker");

        let channel = BChannel::new(worker.port());

        Self { worker, channel }
    }
}
*/

/*
#[wasm_bindgen]
pub fn launch_worker() -> BSharedWorker {
    info!("doing the worker");
    let mut opts = WorkerOptions::new();
    opts.type_(WorkerType::Module);
    let worker =
        SharedWorker::new_with_worker_options("/js/worker.js", &opts).expect("could not worker");

    let port = worker.port();

    info!("did the worker");
}
*/

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

    //launch_worker();
    //create_channel();
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
