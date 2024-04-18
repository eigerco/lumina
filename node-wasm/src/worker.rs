use std::fmt::Debug;
use std::marker::PhantomData;

use libp2p::Multiaddr;
use libp2p::PeerId;
use lumina_node::store::SamplingMetadata;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker};

use celestia_types::ExtendedHeader;
use lumina_node::node::{Node, NodeError};
use lumina_node::store::{IndexedDbStore, Store};
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::utils::WorkerSelf;
use crate::worker::commands::*; // TODO

pub(crate) mod commands;

/// actual type that's sent over a js message port
pub(crate) type WireMessage = Option<Result<WorkerResponse, WorkerError>>;

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
    #[error("Worker is still handling previous command")]
    WorkerBusy,

    #[error("Node hasn't been started yet")]
    NodeNotRunning,
    #[error("Node has already been started")]
    NodeAlreadyRunning,

    #[error("node error: {0}")]
    NodeError(String),

    #[error("could not send command to the worker: {0}")]
    CommandSendingFailed(String),

    #[error("respose to command did not match expected type")]
    InvalidResponseType,

    #[error("response message could not be serialised, should not happen: {0}")]
    CouldNotSerialiseResponse(String),
    #[error("response message could not be sent: {0}")]
    CouldNotSendResponse(String),

    #[error("Received empty worker response, should not happen")]
    EmptyWorkerResponse,

    #[error("Response channel to worker closed, should not happen")]
    ResponseChannelDropped,
}

impl From<NodeError> for WorkerError {
    fn from(error: NodeError) -> Self {
        WorkerError::NodeError(error.to_string())
    }
}

type Result<T, E = WorkerError> = std::result::Result<T, E>;
type WorkerClientConnection = (MessagePort, Closure<dyn Fn(MessageEvent)>);

#[derive(Serialize, Deserialize, Debug)]
pub struct NodeCommandResponse<T>(pub T::Output)
where
    T: NodeCommandType,
    T::Output: Debug + Serialize;

pub trait NodeCommandType: Debug + Into<NodeCommand> {
    type Output;
}

pub struct SharedWorkerChannel<IN> {
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    channel: MessagePort,
    response_channel: mpsc::Receiver<WireMessage>,
    send_type: PhantomData<IN>,
}

impl<IN> SharedWorkerChannel<IN>
where
    IN: Serialize,
{
    pub fn new(channel: MessagePort) -> Self {
        let (response_tx, mut response_rx) = mpsc::channel(64);

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
                    error!("message forwarding channel closed, should not happen");
                }
            })
        };

        // TODO: one of the ways to make sure we're synchronised with lumina in worker is forcing
        // request-response interface over the channel (waiting for previous command to be handled
        // before sending next one). Is it better for lumina to maintain this inside the worker?
        //let (response_tx, mut response_rx) = mpsc::channel(1);

        /*
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
        */

        let onmessage = Closure::new(onmessage_callback);
        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        channel.start();
        Self {
            response_channel: response_rx,
            _onmessage: onmessage,
            channel,
            send_type: PhantomData,
        }
    }

    pub async fn send(&self, command: IN) -> Result<(), WorkerError> {
        let v = to_value(&command)
            .map_err(|e| WorkerError::CouldNotSerialiseResponse(e.to_string()))?;
        self.channel.post_message(&v).map_err(|e| {
            WorkerError::CouldNotSendResponse(e.as_string().unwrap_or("UNDEFINED".to_string()))
        })

        /*
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
        */
    }

    pub async fn recv(&self) -> Result<WorkerResponse, WorkerError> {
        let message: WireMessage = self
            .response_channel
            .recv()
            .await
            .ok_or(WorkerError::ResponseChannelDropped)?;
        message.ok_or(WorkerError::EmptyWorkerResponse)?
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

    async fn get_syncer_info(&mut self) -> Result<SyncingInfo> {
        Ok(self.node.syncer_info().await?)
    }

    async fn set_peer_trust(&mut self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        Ok(self.node.set_peer_trust(peer_id, is_trusted).await?)
    }

    async fn get_connected_peers(&mut self) -> Result<Vec<String>> {
        Ok(self
            .node
            .connected_peers()
            .await?
            .iter()
            .map(|id| id.to_string())
            .collect())
    }

    async fn get_listeners(&mut self) -> Result<Vec<Multiaddr>> {
        Ok(self.node.listeners().await?)
    }

    async fn wait_connected(&mut self, trusted: bool) -> Result<()> {
        if trusted {
            self.node.wait_connected().await?;
        } else {
            self.node.wait_connected_trusted().await?;
        }
        Ok(())
    }

    async fn request_header(&mut self, query: SingleHeaderQuery) -> Result<ExtendedHeader> {
        Ok(match query {
            SingleHeaderQuery::Head => self.node.request_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.request_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.request_header_by_height(height).await,
        }?)
    }

    async fn get_header(&mut self, query: SingleHeaderQuery) -> Result<ExtendedHeader> {
        Ok(match query {
            SingleHeaderQuery::Head => self.node.get_local_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.get_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.get_header_by_height(height).await,
        }?)
    }

    async fn get_verified_headers(
        &mut self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(self.node.request_verified_headers(&from, amount).await?)
    }

    async fn get_headers_range(
        &mut self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(match (start_height, end_height) {
            (None, None) => self.node.get_headers(..).await,
            (Some(start), None) => self.node.get_headers(start..).await,
            (None, Some(end)) => self.node.get_headers(..=end).await,
            (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
        }?)
    }

    async fn get_sampling_metadata(&mut self, height: u64) -> Result<Option<SamplingMetadata>> {
        Ok(self.node.get_sampling_metadata(height).await?)
    }

    async fn process_command(&mut self, command: NodeCommand) -> WorkerResponse {
        match command {
            NodeCommand::IsRunning => WorkerResponse::IsRunning(true),
            NodeCommand::StartNode(_) => {
                WorkerResponse::NodeStarted(Err(WorkerError::NodeAlreadyRunning))
            }
            NodeCommand::GetLocalPeerId => {
                WorkerResponse::LocalPeerId(self.node.local_peer_id().to_string())
            }
            NodeCommand::GetSyncerInfo => WorkerResponse::SyncerInfo(self.get_syncer_info().await),
            NodeCommand::GetPeerTrackerInfo => {
                let peer_tracker_info = self.node.peer_tracker_info();
                WorkerResponse::PeerTrackerInfo(peer_tracker_info)
            }
            NodeCommand::GetNetworkInfo => {
                let network_info = self.node.network_info().await;
                //WorkerResponse::NetworkInfo(network_info)
                todo!()
            }
            NodeCommand::GetConnectedPeers => {
                WorkerResponse::ConnectedPeers(self.get_connected_peers().await)
            }
            NodeCommand::SetPeerTrust {
                peer_id,
                is_trusted,
            } => WorkerResponse::SetPeerTrust(self.set_peer_trust(peer_id, is_trusted).await),
            NodeCommand::WaitConnected { trusted } => {
                WorkerResponse::Connected(self.wait_connected(trusted).await)
            }
            NodeCommand::GetListeners => WorkerResponse::Listeners(self.get_listeners().await),
            NodeCommand::RequestHeader(query) => {
                WorkerResponse::Header(self.request_header(query).await)
            }
            NodeCommand::GetHeader(query) => WorkerResponse::Header(self.get_header(query).await),
            NodeCommand::GetVerifiedHeaders { from, amount } => {
                WorkerResponse::Headers(self.get_verified_headers(&from, amount).await)
            }
            NodeCommand::GetHeadersRange {
                start_height,
                end_height,
            } => WorkerResponse::Headers(self.get_headers_range(start_height, end_height).await),
            NodeCommand::LastSeenNetworkHead => {
                WorkerResponse::LastSeenNetworkHead(self.node.get_network_head_header())
            }
            NodeCommand::GetSamplingMetadata { height } => {
                WorkerResponse::SamplingMetadata(self.get_sampling_metadata(height).await)
            }
        }
    }
}

enum WorkerMessage {
    NewConnection(MessagePort),
    Command((NodeCommand, ClientId)),
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
                    let Ok(command) = from_value::<NodeCommand>(message_data) else {
                        warn!("could not deserialize message from client {client_id}");
                        return;
                    };

                    if let Err(e) = local_tx
                        .send(WorkerMessage::Command((command, ClientId(client_id))))
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

    fn respond_to(&self, client: ClientId, msg: WorkerResponse) {
        self.send_response(client, Ok(msg))
    }

    fn respond_err_to(&self, client: ClientId, error: WorkerError) {
        self.send_response(client, Err(error))
    }

    fn send_response(&self, client: ClientId, msg: Result<WorkerResponse, WorkerError>) {
        let offset = client.0;

        let wire_message: WireMessage = Some(msg);

        let message = match to_value(&wire_message) {
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
            WorkerMessage::Command((command, client_id)) => {
                let Some(worker) = &mut worker else {
                    match command {
                        NodeCommand::IsRunning => {
                            connector.respond_to(client_id, WorkerResponse::IsRunning(false));
                        }
                        NodeCommand::StartNode(config) => {
                            worker = Some(NodeWorker::new(config).await);
                            connector.respond_to(client_id, WorkerResponse::NodeStarted(Ok(())));
                        }
                        _ => {
                            warn!("Worker not running");
                            connector.respond_err_to(client_id, WorkerError::NodeNotRunning);
                        }
                    }
                    continue;
                };

                trace!("received from {client_id:?}: {command:?}");
                worker.process_command(command).await;
            }
        }
    }

    error!("Worker exited, should not happen");
}
