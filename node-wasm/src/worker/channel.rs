use std::fmt::{self, Debug};

use js_sys::JsString;
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker};

use crate::utils::WorkerSelf;
use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::worker::WorkerError;

type WireMessage = Result<WorkerResponse, WorkerError>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

const WORKER_CHANNEL_SIZE: usize = 1;

/// `WorkerClient` is responsible for sending messages to and receiving responses from [`WorkerMessageServer`].
/// It covers JS details like callbacks, having to synchronise requests and responses and exposes
/// simple RPC-like function call interface.
pub(crate) struct WorkerClient {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    message_port: MessagePort,
    response_channel: Mutex<mpsc::Receiver<WireMessage>>,
}

impl WorkerClient {
    /// Create a new `WorkerClient` out of a [`MessagePort`] that should be connected
    /// to a [`SharedWorker`] running lumina.
    ///
    /// [`SharedWorker`]: https://developer.mozilla.org/en-US/docs/Web/API/SharedWorker
    /// [`MessagePort`]: https://developer.mozilla.org/en-US/docs/Web/API/MessagePort
    pub fn new(message_port: MessagePort) -> Self {
        let (response_tx, response_rx) = mpsc::channel(WORKER_CHANNEL_SIZE);

        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = response_tx.clone();
            spawn_local(async move {
                let data: WireMessage = match from_value(ev.data()) {
                    Ok(jsvalue) => jsvalue,
                    Err(e) => {
                        error!("WorkerClient could not convert from JsValue: {e}");
                        Err(WorkerError::CouldNotDeserialiseResponse(e.to_string()))
                    }
                };

                if let Err(e) = response_tx.send(data).await {
                    error!("message forwarding channel closed, should not happen: {e}");
                }
            })
        };

        let onmessage = Closure::new(onmessage_callback);
        message_port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        message_port.start();
        Self {
            response_channel: Mutex::new(response_rx),
            _onmessage: onmessage,
            message_port,
        }
    }

    /// Send command to lumina and wait for a response.
    ///
    /// Response enum variant can be converted into appropriate type at runtime with a provided
    /// [`CheckableResponseExt`] helper.
    ///
    /// [`CheckableResponseExt`]: crate::utils::CheckableResponseExt
    pub async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse, WorkerError> {
        let mut response_channel = self.response_channel.lock().await;
        self.send(command)?;

        let message: WireMessage = response_channel
            .recv()
            .await
            .ok_or(WorkerError::ResponseChannelDropped)?;
        message
    }

    fn send(&self, command: NodeCommand) -> Result<(), WorkerError> {
        let command_value =
            to_value(&command).map_err(|e| WorkerError::CouldNotSerialiseCommand(e.to_string()))?;
        self.message_port
            .post_message(&command_value)
            .map_err(|e| WorkerError::CouldNotSendCommand(format!("{:?}", e.dyn_ref::<JsString>())))
    }
}

#[derive(Debug)]
pub(super) struct ClientId(usize);

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Client({})", self.0)
    }
}

pub(super) enum WorkerMessage {
    NewConnection(MessagePort),
    InvalidCommandReceived(ClientId),
    Command((NodeCommand, ClientId)),
}

pub(super) struct WorkerMessageServer {
    // same onconnect callback is used throughtout entire Worker lifetime.
    // Keep a reference to make sure it doesn't get dropped.
    _onconnect_callback: Closure<dyn Fn(MessageEvent)>,

    // keep a MessagePort for each client to send messages over, as well as callback responsible
    // for forwarding messages back
    clients: Vec<WorkerClientConnection>,

    // sends events back to the main loop for processing
    command_channel: mpsc::Sender<WorkerMessage>,
}

impl WorkerMessageServer {
    pub fn new(command_channel: mpsc::Sender<WorkerMessage>) -> Self {
        let closure_command_channel = command_channel.clone();
        let onconnect: Closure<dyn Fn(MessageEvent)> = Closure::new(move |ev: MessageEvent| {
            let command_channel = closure_command_channel.clone();
            spawn_local(async move {
                let Ok(port) = ev.ports().at(0).dyn_into() else {
                    error!("received onconnect event without MessagePort, should not happen");
                    return;
                };

                if let Err(e) = command_channel
                    .send(WorkerMessage::NewConnection(port))
                    .await
                {
                    error!("command channel inside worker closed, should not happen: {e}");
                }
            })
        });

        let worker_scope = SharedWorker::worker_self();
        worker_scope.set_onconnect(Some(onconnect.as_ref().unchecked_ref()));

        Self {
            _onconnect_callback: onconnect,
            clients: Vec::with_capacity(1), // we usually expect to have exactly one client
            command_channel,
        }
    }

    pub fn respond_to(&self, client: ClientId, msg: WorkerResponse) {
        self.send_response(client, Ok(msg))
    }

    pub fn respond_err_to(&self, client: ClientId, error: WorkerError) {
        self.send_response(client, Err(error))
    }

    pub fn add(&mut self, port: MessagePort) {
        let client_id = self.clients.len();

        let near_tx = self.command_channel.clone();
        let client_message_callback: Closure<dyn Fn(MessageEvent)> =
            Closure::new(move |ev: MessageEvent| {
                let local_tx = near_tx.clone();
                spawn_local(async move {
                    let client_id = ClientId(client_id);

                    let message = match from_value(ev.data()) {
                        Ok(command) => {
                            debug!("received command from client {client_id}: {command:#?}");
                            WorkerMessage::Command((command, client_id))
                        }
                        Err(e) => {
                            warn!("could not deserialize message from client {client_id}: {e}");
                            WorkerMessage::InvalidCommandReceived(client_id)
                        }
                    };

                    if let Err(e) = local_tx.send(message).await {
                        error!("command channel inside worker closed, should not happen: {e}");
                    }
                })
            });

        self.clients.push((port, client_message_callback));

        let (port, callback) = self.clients.last().unwrap();
        port.set_onmessage(Some(callback.as_ref().unchecked_ref()));

        info!("SharedWorker ready to receive commands from client {client_id}");
    }

    fn send_response(&self, client: ClientId, message: WireMessage) {
        let message = match to_value(&message) {
            Ok(jsvalue) => jsvalue,
            Err(e) => {
                warn!("provided response could not be coverted to JsValue: {e}");
                to_value(&WorkerError::CouldNotSerialiseResponse(e.to_string()))
                    .expect("something's wrong, couldn't serialise serialisation error")
            }
        };

        let Some((client_port, _)) = self.clients.get(client.0) else {
            error!("client {client} not found on client list, should not happen");
            return;
        };

        if let Err(e) = client_port.post_message(&message) {
            error!("could not post response message to client {client}: {e:?}");
        }
    }
}
