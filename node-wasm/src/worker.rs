use std::fmt::Debug;
use std::marker::PhantomData;

use futures::future::{BoxFuture, Future, TryFutureExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, trace, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker};

use lumina_node::node::{Node, NodeError};
use lumina_node::store::{IndexedDbStore, Store};

use crate::node::WasmNodeConfig;
use crate::utils::WorkerSelf;
use crate::worker::commands::*;

pub(crate) mod commands;

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum WorkerError {
    #[error("Command response channel has been dropped before response could be sent, should not happen")]
    SharedWorkerChannelResponseChannelDropped,
    #[error("Command channel to lumina worker closed, should not happen")]
    SharedWorkerCommandChannelClosed,
    #[error("Channel expected different response type, should not happen")]
    SharedWorkerChannelInvalidType,
    #[error("Lumina is already running, ignoring StartNode command")]
    SharedWorkerAlreadyRunning,
    #[error("node error: {0}")]
    NodeError(String),
    #[error("Worker is still handling previous command")]
    WorkerBusy,
}

impl From<NodeError> for WorkerError {
    fn from(error: NodeError) -> Self {
        WorkerError::NodeError(error.to_string())
    }
}

type Result<T, E = WorkerError> = std::result::Result<T, E>;
pub type CommandResponseChannel<T> = oneshot::Sender<<T as NodeCommandType>::Output>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

#[derive(Serialize, Deserialize, Debug)]
pub struct NodeCommandResponse<T>(pub T::Output)
where
    T: NodeCommandType,
    T::Output: Debug + Serialize;

pub trait NodeCommandType: Debug + Into<NodeCommand> {
    type Output;
}

pub struct SharedWorkerChannel<IN, OUT> {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    channel: MessagePort,
    response_channels: mpsc::Sender<oneshot::Sender<OUT>>,
    send_type: PhantomData<IN>,
}

impl<IN, OUT> SharedWorkerChannel<IN, OUT>
where
    IN: Serialize,
    OUT: DeserializeOwned + 'static,
{
    pub fn new(channel: MessagePort) -> Self {
        let (message_tx, mut message_rx) = mpsc::channel(64);

        let near_tx = message_tx.clone();
        let onmessage_callback = move |ev: MessageEvent| {
            let local_tx = near_tx.clone();
            spawn_local(async move {
                let message_data = ev.data();

                let data = from_value(message_data).unwrap();

                local_tx.send(data).await.expect("send err");
            })
        };

        // TODO: one of the ways to make sure we're synchronised with lumina in worker is forcing
        // request-response interface over the channel (waiting for previous command to be handled
        // before sending next one). Is it better for lumina to maintain this inside the worker?
        let (response_tx, mut response_rx) = mpsc::channel(1);

        // small task running concurently, which converts js's callback style into
        // rust's awaiting on a channel message passing style
        spawn_local(async move {
            loop {
                let message = message_rx.recv().await.unwrap();
                let response_channel: oneshot::Sender<OUT> = response_rx.recv().await.unwrap();
                if response_channel.send(message).is_err() {
                    warn!(
                        "response channel closed before response could be sent, dropping message"
                    );
                }
            }
        });

        let onmessage = Closure::new(onmessage_callback);
        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        channel.start();
        Self {
            response_channels: response_tx,
            _onmessage: onmessage,
            channel,
            send_type: PhantomData,
        }
    }

    pub async fn send<T: NodeCommandType>(
        &self,
        command: T,
    ) -> Result<impl Future<Output = Result<T::Output>>>
    where
        T::Output: Debug + Serialize,
        NodeCommandResponse<T>: TryFrom<OUT>,
        <NodeCommandResponse<T> as TryFrom<OUT>>::Error: Debug,
    {
        self.send_enum(command.into());

        let (tx, rx) = oneshot::channel();

        self.response_channels
            .send(tx)
            .await
            .map_err(|_| WorkerError::SharedWorkerCommandChannelClosed)?;

        Ok(rx.map_ok_or_else(
            |_e| Err(WorkerError::SharedWorkerChannelResponseChannelDropped),
            |r| match NodeCommandResponse::<T>::try_from(r) {
                Ok(v) => Ok(v.0),
                Err(_) => Err(WorkerError::SharedWorkerChannelInvalidType),
            },
        ))
    }

    fn send_enum(&self, msg: NodeCommand) {
        let v = to_value(&msg).unwrap();
        self.channel.post_message(&v).expect("err post");
    }
}

// TODO: cleanup JS objects on drop
// impl Drop

struct NodeWorker {
    node: Node<IndexedDbStore>,
}

impl NodeWorker {
    async fn new(config: WasmNodeConfig) -> Self {
        let config = config.into_node_config().await.ok().unwrap();

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialized new empty store");
        }

        let node = Node::new(config).await.ok().unwrap();

        Self { node }
    }

    async fn process_command(&mut self, command: NodeCommandWithChannel) {
        match command {
            NodeCommandWithChannel::IsRunning((_, response)) => {
                response.send(true).expect("channel_dropped")
            }
            NodeCommandWithChannel::StartNode((_, response)) => {
                response
                    .send(Err(WorkerError::SharedWorkerAlreadyRunning))
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::GetLocalPeerId((_, response)) => {
                response
                    .send(self.node.local_peer_id().to_string())
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::GetSyncerInfo((_, response)) => {
                let syncer_info = self.node.syncer_info().await.unwrap();
                response.send(syncer_info).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetPeerTrackerInfo((_, response)) => {
                let peer_tracker_info = self.node.peer_tracker_info();
                response.send(peer_tracker_info).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetNetworkInfo((_, response)) => {
                let network_info = self.node.network_info().await.expect("TODO").into();
                response.send(network_info).expect("channel_dropped")
            }
            NodeCommandWithChannel::GetConnectedPeers((_, response)) => {
                let connected_peers = self.node.connected_peers().await.expect("TODO");
                response
                    .send(connected_peers.iter().map(|id| id.to_string()).collect())
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::SetPeerTrust((
                SetPeerTrust {
                    peer_id,
                    is_trusted,
                },
                response,
            )) => {
                self.node
                    .set_peer_trust(peer_id.parse().unwrap(), is_trusted)
                    .await
                    .unwrap();
                response.send(()).expect("channel_dropped");
            }
            NodeCommandWithChannel::WaitConnected((parameters, response)) => {
                // TODO: nonblocking on channels
                if parameters.trusted {
                    let _ = self.node.wait_connected().await;
                } else {
                    let _ = self.node.wait_connected_trusted().await;
                }
                response.send(()).expect("channel_dropped")
            }
            NodeCommandWithChannel::GetListeners((_, response)) => response
                .send(self.node.listeners().await.unwrap())
                .expect("channel_dropped"),
            NodeCommandWithChannel::RequestHeader((command, response)) => {
                let header = match command.0 {
                    SingleHeaderQuery::Head => self.node.request_head_header().await,
                    SingleHeaderQuery::ByHash(hash) => {
                        self.node.request_header_by_hash(&hash).await
                    }
                    SingleHeaderQuery::ByHeight(height) => {
                        self.node.request_header_by_height(height).await
                    }
                };
                response
                    .send(header.map_err(|e| e.into()))
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::GetHeader((command, response)) => {
                let header = match command.0 {
                    SingleHeaderQuery::Head => self.node.get_local_head_header().await.unwrap(),
                    SingleHeaderQuery::ByHash(hash) => {
                        self.node.get_header_by_hash(&hash).await.ok().unwrap()
                    }
                    SingleHeaderQuery::ByHeight(height) => {
                        self.node.get_header_by_height(height).await.ok().unwrap()
                    }
                };
                response.send(header).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetVerifiedHeaders((command, response)) => {
                let GetVerifiedHeaders { from, amount } = command;
                let headers = self
                    .node
                    .request_verified_headers(&from, amount)
                    .await
                    .unwrap();
                response.send(headers).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetHeadersRange((command, response)) => {
                let GetHeadersRange {
                    start_height,
                    end_height,
                } = command;
                let headers = match (start_height, end_height) {
                    (None, None) => self.node.get_headers(..).await,
                    (Some(start), None) => self.node.get_headers(start..).await,
                    (None, Some(end)) => self.node.get_headers(..=end).await,
                    (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
                }
                .ok()
                .unwrap();
                response.send(headers).expect("channel_dropped");
            }
            NodeCommandWithChannel::LastSeenNetworkHead((_, response)) => {
                let header = self.node.get_network_head_header();
                response.send(header).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetSamplingMetadata((command, response)) => {
                let metadata = self
                    .node
                    .get_sampling_metadata(command.height)
                    .await
                    .ok()
                    .unwrap()
                    .unwrap();
                response.send(metadata).expect("channel_dropped");
            }
        };
    }
}

enum WorkerMessage {
    NewConnection(MessagePort),
    Command(NodeCommandWithChannel),
    ResponseChannel(
        (
            ClientId,
            BoxFuture<'static, Result<NodeResponse, tokio::sync::oneshot::error::RecvError>>,
        ),
    ),
}

#[derive(Debug)]
struct ClientId(usize);

struct WorkerConnector {
    // same onconnect callback is used throughtout entire Worker lifetime.
    // Keep a reference to make sure it doesn't get dropped.
    _onconnect_callback: Closure<dyn Fn(MessageEvent)>,

    // keep a MessagePort for each client to send messages over, as well as callback responsible
    // for forwarding messages back
    clients: Vec<WorkerClientConnection>,

    // sends events back to the main loop for processing
    command_channel: mpsc::Sender<WorkerMessage>,
}

impl WorkerConnector {
    fn new(command_channel: mpsc::Sender<WorkerMessage>) -> Self {
        let worker_scope = SharedWorker::worker_self();

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

        worker_scope.set_onconnect(Some(onconnect_callback.as_ref().unchecked_ref()));

        Self {
            _onconnect_callback: onconnect_callback,
            clients: Vec::with_capacity(1), // we usually expect to have exactly one client
            command_channel,
        }
    }

    fn add(&mut self, port: MessagePort) {
        let client_id = self.clients.len();

        let near_tx = self.command_channel.clone();
        let client_message_callback: Closure<dyn Fn(MessageEvent)> =
            Closure::new(move |ev: MessageEvent| {
                let local_tx = near_tx.clone();
                spawn_local(async move {
                    let message_data = ev.data();
                    let Ok(node_command) = from_value::<NodeCommand>(message_data) else {
                        warn!("could not deserialize message from client {client_id}");
                        return;
                    };
                    let (command_with_channel, response_channel) =
                        node_command.add_response_channel();

                    if let Err(e) = local_tx
                        .send(WorkerMessage::Command(command_with_channel))
                        .await
                    {
                        error!("command channel inside worker closed, should not happen: {e}");
                    }

                    // TODO: something cleaner?
                    local_tx
                        .send(WorkerMessage::ResponseChannel((
                            ClientId(client_id),
                            response_channel,
                        )))
                        .await
                        .expect("send4 err");
                })
            });
        port.set_onmessage(Some(client_message_callback.as_ref().unchecked_ref()));

        self.clients.push((port, client_message_callback));

        info!("SharedWorker ready to receive commands from client {client_id}");
    }

    fn respond_to(&self, client: ClientId, msg: Option<NodeResponse>) {
        let offset = client.0;
        let message = match to_value(&msg) {
            Ok(jsvalue) => jsvalue,
            Err(e) => {
                warn!("provided response could not be coverted to JsValue: {e}, sending undefined instead");
                JsValue::UNDEFINED // we need to send something, client is waiting for a response
            }
        };

        // XXX defensive programming with array checking?
        if let Err(e) = self.clients[offset].0.post_message(&message) {
            error!("could not post response message to client {client:?}: {e:?}");
        }
    }
}

#[wasm_bindgen]
pub async fn run_worker(queued_connections: Vec<MessagePort>) {
    let (tx, mut rx) = mpsc::channel(64);
    let mut connector = WorkerConnector::new(tx.clone());

    for connection in queued_connections {
        connector.add(connection);
    }

    let mut worker = None;
    while let Some(message) = rx.recv().await {
        match message {
            WorkerMessage::NewConnection(connection) => {
                connector.add(connection);
            }
            WorkerMessage::Command(command_with_channel) => {
                let Some(worker) = &mut worker else {
                    match command_with_channel {
                        NodeCommandWithChannel::IsRunning((_, response)) => {
                            response.send(false).expect("channel_dropped");
                        }
                        NodeCommandWithChannel::StartNode((command, response)) => {
                            worker = Some(NodeWorker::new(command.0).await);
                            response.send(Ok(())).expect("channel_dropped");
                        }
                        _ => warn!("Worker not running"),
                    }
                    continue;
                };

                trace!("received: {command_with_channel:?}");
                worker.process_command(command_with_channel).await;
            }
            WorkerMessage::ResponseChannel((client_id, channel)) => {
                // oneshot channel has one failure condition - Sender getting dropped
                // we also _need_ to send some response, otherwise driver gets stuck waiting
                let response = channel.await.ok();
                connector.respond_to(client_id, response);
            }
        }
    }

    error!("Worker exited, should not happen");
}
