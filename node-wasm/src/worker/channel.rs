use std::fmt::{self, Debug};

use js_sys::JsString;
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, MessagePort, SharedWorker, Worker};

use crate::utils::WorkerSelf;
use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::worker::WorkerError;

type WireMessage = Result<WorkerResponse, WorkerError>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

/// Access to sending channel is protected by mutex to make sure we only can hold a single
/// writable instance from JS. Thus we expect to have at most 1 message in-flight.
const WORKER_CHANNEL_SIZE: usize = 1;

/// `WorkerClient` is responsible for sending messages to and receiving responses from [`WorkerMessageServer`].
/// It covers JS details like callbacks, having to synchronise requests and responses and exposes
/// simple RPC-like function call interface.
pub(crate) struct WorkerClient {
    worker: AnyWorker,
    response_channel: Mutex<mpsc::Receiver<WireMessage>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _onerror: Closure<dyn Fn(MessageEvent)>,
}

impl From<Worker> for WorkerClient {
    fn from(worker: Worker) -> WorkerClient {
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
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let onerror = Closure::new(|ev: MessageEvent| {
            error!("received error from Worker: {:?}", ev.to_string());
        });
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        Self {
            worker: AnyWorker::DedicatedWorker(worker),
            response_channel: Mutex::new(response_rx),
            _onmessage: onmessage,
            _onerror: onerror,
        }
    }
}

impl From<SharedWorker> for WorkerClient {
    fn from(worker: SharedWorker) -> WorkerClient {
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
        let message_port = worker.port();
        message_port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let onerror: Closure<dyn Fn(MessageEvent)> = Closure::new(|ev: MessageEvent| {
            error!("received error from SharedWorker: {:?}", ev.to_string());
        });
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        message_port.start();
        Self {
            worker: AnyWorker::SharedWorker(worker),
            response_channel: Mutex::new(response_rx),
            _onmessage: onmessage,
            _onerror: onerror,
        }
    }
}

enum AnyWorker {
    DedicatedWorker(Worker),
    SharedWorker(SharedWorker),
}

impl WorkerClient {
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
        match &self.worker {
            AnyWorker::DedicatedWorker(worker) => {
                worker.post_message(&command_value).map_err(|e| {
                    WorkerError::CouldNotSendCommand(format!("{:?}", e.dyn_ref::<JsString>()))
                })
            }
            AnyWorker::SharedWorker(worker) => {
                worker.port().post_message(&command_value).map_err(|e| {
                    WorkerError::CouldNotSendCommand(format!("{:?}", e.dyn_ref::<JsString>()))
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
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

pub(super) trait MessageServer {
    fn send_response(&self, client: ClientId, message: WireMessage);
    fn add(&mut self, port: MessagePort);

    fn respond_to(&self, client: ClientId, msg: WorkerResponse) {
        self.send_response(client, Ok(msg))
    }

    fn respond_err_to(&self, client: ClientId, error: WorkerError) {
        self.send_response(client, Err(error))
    }
}

pub(super) struct SharedWorkerMessageServer {
    // same onconnect callback is used throughtout entire Worker lifetime.
    // Keep a reference to make sure it doesn't get dropped.
    _onconnect: Closure<dyn Fn(MessageEvent)>,

    // keep a MessagePort for each client to send messages over, as well as callback responsible
    // for forwarding messages back
    clients: Vec<WorkerClientConnection>,

    // sends events back to the main loop for processing
    command_channel: mpsc::Sender<WorkerMessage>,
}

impl SharedWorkerMessageServer {
    pub fn new(command_channel: mpsc::Sender<WorkerMessage>, queued: Vec<MessageEvent>) -> Self {
        let worker_scope = SharedWorker::worker_self();
        let onconnect = get_client_connect_callback(command_channel.clone());
        worker_scope.set_onconnect(Some(onconnect.as_ref().unchecked_ref()));

        let mut server = Self {
            _onconnect: onconnect,
            clients: Vec::with_capacity(1), // we usually expect to have exactly one client
            command_channel,
        };

        for event in queued {
            if let Ok(port) = event.ports().at(0).dyn_into() {
                server.add(port);
            } else {
                error!("received onconnect event without MessagePort, should not happen");
            }
        }

        server
    }
}

impl MessageServer for SharedWorkerMessageServer {
    fn add(&mut self, port: MessagePort) {
        let client_id = ClientId(self.clients.len());

        let client_message_callback =
            get_client_message_callback(self.command_channel.clone(), client_id);

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

pub(super) struct DedicatedWorkerMessageServer {
    // same onmessage callback is used throughtout entire Worker lifetime.
    // Keep a reference to make sure it doesn't get dropped.
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    // global scope we use to send messages
    worker: DedicatedWorkerGlobalScope,
}

impl DedicatedWorkerMessageServer {
    pub async fn new(
        command_channel: mpsc::Sender<WorkerMessage>,
        queued: Vec<MessageEvent>,
    ) -> Self {
        for event in queued {
            let message = parse_message_event_to_worker_message(event, ClientId(0));

            if let Err(e) = command_channel.send(message).await {
                error!("command channel inside worker closed, should not happen: {e}");
            }
        }

        let worker = Worker::worker_self();
        let onmessage = get_client_message_callback(command_channel, ClientId(0));
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Self {
            _onmessage: onmessage,
            worker,
        }
    }
}

impl MessageServer for DedicatedWorkerMessageServer {
    fn add(&mut self, _port: MessagePort) {
        warn!("DedicatedWorkerMessageServer::add called, should not happen");
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

        if let Err(e) = self.worker.post_message(&message) {
            error!("could not post response message to client {client}: {e:?}");
        }
    }
}

fn get_client_connect_callback(
    command_channel: mpsc::Sender<WorkerMessage>,
) -> Closure<dyn Fn(MessageEvent)> {
    Closure::new(move |ev: MessageEvent| {
        let command_channel = command_channel.clone();
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
    })
}

fn get_client_message_callback(
    command_channel: mpsc::Sender<WorkerMessage>,
    client: ClientId,
) -> Closure<dyn Fn(MessageEvent)> {
    Closure::new(move |ev: MessageEvent| {
        let command_channel = command_channel.clone();
        spawn_local(async move {
            let message = parse_message_event_to_worker_message(ev, client);

            if let Err(e) = command_channel.send(message).await {
                error!("command channel inside worker closed, should not happen: {e}");
            }
        })
    })
}

fn parse_message_event_to_worker_message(ev: MessageEvent, client: ClientId) -> WorkerMessage {
    match from_value(ev.data()) {
        Ok(command) => {
            debug!("received command from client {client}: {command:#?}");
            WorkerMessage::Command((command, client))
        }
        Err(e) => {
            warn!("could not deserialize message from client {client}: {e}");
            WorkerMessage::InvalidCommandReceived(client)
        }
    }
}
