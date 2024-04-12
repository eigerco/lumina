//! Various utilities for interacting with node from wasm.

use std::fmt::{self, Debug};
use std::marker::PhantomData;

use futures::future::BoxFuture;
use futures::FutureExt;
use futures::TryFutureExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, oneshot};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker, SharedWorkerGlobalScope};

use lumina_node::network;

use crate::worker::NodeCommand;

pub type CommandResponseChannel<T> = oneshot::Sender<<T as NodeCommandType>::Output>;
#[derive(Serialize, Deserialize, Debug)]
pub struct NodeCommandResponse<T>(pub T::Output)
where
    T: NodeCommandType,
    T::Output: Debug + Serialize;

pub trait NodeCommandType: Debug + Into<NodeCommand> {
    type Output;
}

pub struct BChannel<IN, OUT> {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _forwarding_task: (),
    channel: MessagePort,
    response_channels: mpsc::Sender<oneshot::Sender<OUT>>,
    send_type: PhantomData<IN>,
}

impl<IN, OUT> BChannel<IN, OUT>
where
    IN: Serialize,
    // XXX: send sync shouldn't be needed
    OUT: DeserializeOwned + 'static + Send + Sync,
{
    pub fn new(channel: MessagePort) -> Self {
        let (tx, mut message_rx) = mpsc::channel(64);

        let near_tx = tx.clone();
        let f = move |ev: MessageEvent| {
            let local_tx = near_tx.clone();
            spawn_local(async move {
                let message_data = ev.data();

                let data = from_value(message_data).unwrap();

                local_tx.send(data).await.expect("send err");
            })
        };

        let (response_tx, mut response_rx) = mpsc::channel(1);
        let forwarding_task = spawn_local(async move {
            loop {
                let message = message_rx.recv().await.unwrap();
                let response_channel: oneshot::Sender<OUT> = response_rx.recv().await.unwrap();
                response_channel
                    .send(message)
                    .ok()
                    .expect("forwarding broke");
            }
        });

        let onmessage = Closure::new(f);

        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        channel.start();
        Self {
            response_channels: response_tx,
            _onmessage: onmessage,
            _forwarding_task: forwarding_task,
            channel,
            send_type: PhantomData,
        }
    }

    pub fn send<T: NodeCommandType>(
        &self,
        command: T,
    ) -> BoxFuture<'static, Result<T::Output, tokio::sync::oneshot::error::RecvError>>
    where
        T::Output: Debug + Serialize,
        NodeCommandResponse<T>: TryFrom<OUT>,
        <NodeCommandResponse<T> as TryFrom<OUT>>::Error: Debug,
    {
        let message: NodeCommand = command.into();
        self.send_enum(message);

        let (tx, rx) = oneshot::channel();

        self.response_channels.try_send(tx).expect("busy");

        // unwrap "handles" invalid type
        rx.map_ok(|r| {
            tracing::info!("type = , expected = {}", std::any::type_name::<T>());
            NodeCommandResponse::<T>::try_from(r)
                .expect("invalid type")
                .0
        })
        .boxed()
    }

    fn send_enum(&self, msg: NodeCommand) {
        let v = to_value(&msg).unwrap();
        self.channel.post_message(&v).expect("err post");
    }
}

fn string_type_of<T>(_: &T) -> String {
    format!("{}", std::any::type_name::<T>())
}

// TODO: cleanup JS objects on drop
// impl Drop

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
