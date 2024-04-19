use std::fmt::{self, Debug};

use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker};

use crate::utils::WorkerSelf;
use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::worker::WorkerError;

type WireMessage = Option<Result<WorkerResponse, WorkerError>>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

// TODO: cleanup JS objects on drop
// impl Drop
pub(crate) struct WorkerClient {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    channel: MessagePort,
    response_channel: Mutex<mpsc::Receiver<WireMessage>>,
}

impl WorkerClient {
    pub fn new(channel: MessagePort) -> Self {
        let (response_tx, response_rx) = mpsc::channel(64);

        let near_tx = response_tx.clone();
        let onmessage_callback = move |ev: MessageEvent| {
            let local_tx = near_tx.clone();
            spawn_local(async move {
                let message_data = ev.data();

                let data = from_value(message_data)
                    .map_err(|e| {
                        error!("could not convert from JsValue: {e}");
                    })
                    .ok();

                if let Err(e) = local_tx.send(data).await {
                    error!("message forwarding channel closed, should not happen: {e}");
                }
            })
        };

        let onmessage = Closure::new(onmessage_callback);
        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        channel.start();
        Self {
            response_channel: Mutex::new(response_rx),
            _onmessage: onmessage,
            channel,
        }
    }

    pub async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse, WorkerError> {
        let mut channel = self.response_channel.lock().await;
        self.send(command)?;

        let message: WireMessage = channel
            .recv()
            .await
            .ok_or(WorkerError::ResponseChannelDropped)?;
        message.ok_or(WorkerError::EmptyWorkerResponse)?
    }

    fn send(&self, command: NodeCommand) -> Result<(), WorkerError> {
        let command_value =
            to_value(&command).map_err(|e| WorkerError::CouldNotSerialiseCommand(e.to_string()))?;
        self.channel.post_message(&command_value).map_err(|e| {
            WorkerError::CouldNotSendCommand(e.as_string().unwrap_or("UNDEFINED".to_string()))
        })
    }
}

#[derive(Debug)]
pub(super) struct ClientId(usize);

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Client({})", self.0)
    }
}

#[allow(clippy::large_enum_variant)]
pub(super) enum WorkerMessage {
    NewConnection(MessagePort),
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
        let near_tx = command_channel.clone();
        let onconnect_callback: Closure<dyn Fn(MessageEvent)> =
            Closure::new(move |ev: MessageEvent| {
                let local_tx = near_tx.clone();
                spawn_local(async move {
                    let Ok(port) = ev.ports().at(0).dyn_into() else {
                        error!("received connection event without MessagePort, should not happen");
                        return;
                    };

                    if let Err(e) = local_tx.send(WorkerMessage::NewConnection(port)).await {
                        error!("command channel inside worker closed, should not happen: {e}");
                    }
                })
            });

        let worker_scope = SharedWorker::worker_self();
        worker_scope.set_onconnect(Some(onconnect_callback.as_ref().unchecked_ref()));

        Self {
            _onconnect_callback: onconnect_callback,
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
                    let message_data = ev.data();
                    let Ok(command) = from_value::<NodeCommand>(message_data) else {
                        warn!("could not deserialize message from client {client_id}");
                        return;
                    };

                    debug!("received command from client {client_id}: {command:#?}");
                    if let Err(e) = local_tx
                        .send(WorkerMessage::Command((command, client_id)))
                        .await
                    {
                        error!("command channel inside worker closed, should not happen: {e}");
                    }
                })
            });
        port.set_onmessage(Some(client_message_callback.as_ref().unchecked_ref()));

        self.clients.push((port, client_message_callback));

        info!("SharedWorker ready to receive commands from client {client_id}");
    }

    fn send_response(&self, client: ClientId, msg: Result<WorkerResponse, WorkerError>) {
        let wire_message: WireMessage = Some(msg);

        let message = match to_value(&wire_message) {
            Ok(jsvalue) => jsvalue,
            Err(e) => {
                warn!("provided response could not be coverted to JsValue: {e}, sending undefined instead");
                JsValue::UNDEFINED // we need to send something, client is waiting for a response
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
