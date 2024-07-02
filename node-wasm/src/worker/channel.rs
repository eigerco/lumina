use std::fmt::{self, Debug};

use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{
    DedicatedWorkerGlobalScope, MessageEvent, MessagePort, SharedWorker, Worker, WorkerOptions,
    WorkerType,
};

use crate::error::{Context, Error, Result};
use crate::node::NodeWorkerKind;
use crate::utils::WorkerSelf;
use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::worker::WorkerError;

type WireMessage = Result<WorkerResponse, WorkerError>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

/// Access to sending channel is protected by mutex to make sure we only can hold a single
/// writable instance from JS. Thus we expect to have at most 1 message in-flight.
const WORKER_CHANNEL_SIZE: usize = 1;

/// `WorkerClient` is responsible for sending messages to and receiving responses from [`WorkerMessageServer`].
/// It covers JS details like callbacks, having to synchronise requests, responses, and exposes
/// simple RPC-like function call interface.
pub(crate) struct WorkerClient {
    worker: AnyWorker,
    response_channel: Mutex<mpsc::Receiver<WireMessage>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _onerror: Closure<dyn Fn(MessageEvent)>,
}

impl WorkerClient {
    /// Create a new WorkerClient to control newly created Shared or Dedicated Worker running
    /// MessageServer
    pub(crate) fn new(worker: AnyWorker) -> Self {
        let (response_tx, response_rx) = mpsc::channel(WORKER_CHANNEL_SIZE);

        let onmessage = worker.setup_on_message_callback(response_tx);
        let onerror = worker.setup_on_error_callback();

        Self {
            worker,
            response_channel: Mutex::new(response_rx),
            _onmessage: onmessage,
            _onerror: onerror,
        }
    }

    /// Send command to lumina and wait for a response.
    ///
    /// Response enum variant can be converted into appropriate type at runtime with a provided
    /// [`CheckableResponseExt`] helper.
    ///
    /// [`CheckableResponseExt`]: crate::utils::CheckableResponseExt
    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse, WorkerError> {
        let mut response_channel = self.response_channel.lock().await;
        self.send(command)
            .map_err(WorkerError::WorkerCommunicationError)?;

        let message: WireMessage = response_channel
            .recv()
            .await
            .expect("response channel should never be dropped");

        message
    }

    fn send(&self, command: NodeCommand) -> Result<()> {
        let command_value =
            to_value(&command).context("could not serialise worker command to be sent")?;
        match &self.worker {
            AnyWorker::DedicatedWorker(worker) => worker
                .post_message(&command_value)
                .context("could not send command to worker"),
            AnyWorker::SharedWorker(worker) => worker
                .port()
                .post_message(&command_value)
                .context("could not send command to worker"),
        }
    }
}

pub(crate) enum AnyWorker {
    DedicatedWorker(Worker),
    SharedWorker(SharedWorker),
}

impl From<SharedWorker> for AnyWorker {
    fn from(worker: SharedWorker) -> Self {
        AnyWorker::SharedWorker(worker)
    }
}

impl From<Worker> for AnyWorker {
    fn from(worker: Worker) -> Self {
        AnyWorker::DedicatedWorker(worker)
    }
}

impl AnyWorker {
    pub(crate) fn new(kind: NodeWorkerKind, url: &str, name: &str) -> Result<Self> {
        let mut opts = WorkerOptions::new();
        opts.type_(WorkerType::Module);
        opts.name(name);

        Ok(match kind {
            NodeWorkerKind::Shared => {
                info!("Starting SharedWorker");
                AnyWorker::SharedWorker(
                    SharedWorker::new_with_worker_options(url, &opts)
                        .context("could not create SharedWorker")?,
                )
            }
            NodeWorkerKind::Dedicated => {
                info!("Starting Worker");
                AnyWorker::DedicatedWorker(
                    Worker::new_with_options(url, &opts).context("could not create Worker")?,
                )
            }
        })
    }

    fn setup_on_message_callback(
        &self,
        response_tx: mpsc::Sender<WireMessage>,
    ) -> Closure<dyn Fn(MessageEvent)> {
        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = response_tx.clone();
            spawn_local(async move {
                let data: WireMessage = match from_value(ev.data()) {
                    Ok(jsvalue) => jsvalue,
                    Err(e) => {
                        error!("WorkerClient could not convert from JsValue: {e}");
                        let error = Error::from(e).context("could not deserialise worker response");
                        Err(WorkerError::WorkerCommunicationError(error))
                    }
                };

                if let Err(e) = response_tx.send(data).await {
                    error!("message forwarding channel closed, should not happen: {e}");
                }
            })
        };

        let onmessage = Closure::new(onmessage_callback);
        match self {
            AnyWorker::SharedWorker(worker) => {
                let message_port = worker.port();
                message_port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            }
            AnyWorker::DedicatedWorker(worker) => {
                worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()))
            }
        }
        onmessage
    }

    fn setup_on_error_callback(&self) -> Closure<dyn Fn(MessageEvent)> {
        let onerror = Closure::new(|ev: MessageEvent| {
            error!("received error from Worker: {:?}", Error::from_js_value(ev));
        });
        match self {
            AnyWorker::SharedWorker(worker) => {
                worker.set_onerror(Some(onerror.as_ref().unchecked_ref()))
            }
            AnyWorker::DedicatedWorker(worker) => {
                worker.set_onerror(Some(onerror.as_ref().unchecked_ref()))
            }
        }

        onerror
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

impl From<(MessageEvent, ClientId)> for WorkerMessage {
    fn from(value: (MessageEvent, ClientId)) -> Self {
        let (event, client) = value;
        match from_value(event.data()) {
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
            clients: Vec::with_capacity(usize::max(queued.len(), 1)),
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
        let Some((client_port, _)) = self.clients.get(client.0) else {
            error!("client {client} not found on the client list, should not happen");
            return;
        };

        if let Err(e) = client_port.post_message(&serialize_response_message(&message)) {
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
            let message = WorkerMessage::from((event, ClientId(0)));

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
        error!("DedicatedWorkerMessageServer::add called, should not happen");
    }

    fn send_response(&self, client: ClientId, message: WireMessage) {
        if let Err(e) = self
            .worker
            .post_message(&serialize_response_message(&message))
        {
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
            let message = WorkerMessage::from((ev, client));

            if let Err(e) = command_channel.send(message).await {
                error!("command channel inside worker closed, should not happen: {e}");
            }
        })
    })
}

fn serialize_response_message(message: &WireMessage) -> JsValue {
    match to_value(message) {
        Ok(jsvalue) => jsvalue,
        Err(e) => {
            warn!("provided response could not be coverted to JsValue: {e}");
            let error = Error::from(e).context("couldn't serialise worker response");
            to_value(&WorkerError::WorkerCommunicationError(error))
                .expect("something's very wrong, couldn't serialise serialisation error")
        }
    }
}
