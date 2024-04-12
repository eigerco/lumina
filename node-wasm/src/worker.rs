use std::error::Error;
use std::fmt::{self, Debug, Display};

use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, trace, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort, SharedWorker};

use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use lumina_node::node::{Node, NodeError};
use lumina_node::peer_tracker::PeerTrackerInfo;
use lumina_node::store::{IndexedDbStore, SamplingMetadata, Store};
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::utils::{CommandResponseChannel, NodeCommandResponse, NodeCommandType, WorkerSelf};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

type Result<T, E = WorkerError> = std::result::Result<T, E>;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerError(String);

impl Error for WorkerError {}
impl Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkerError({})", self.0)
    }
}

impl From<NodeError> for WorkerError {
    fn from(error: NodeError) -> WorkerError {
        WorkerError(error.to_string())
    }
}

macro_rules! define_command_from_impl {
    ($common_name:ident, $command_name:ident) => {
        impl From<$command_name> for $common_name {
            fn from(command: $command_name) -> Self {
                $common_name::$command_name(command)
            }
        }
    };
}

macro_rules! define_command_type_impl {
    ($common_type:ident, $command_name:ident, $output:ty) => {
        impl $common_type for $command_name {
            type Output = $output;
        }
    };
}

macro_rules! define_response_try_from_impl {
    ($common_type:ident, $helper_type:ident, $command_name:ident) => {
        impl TryFrom<$common_type> for $helper_type<$command_name> {
            type Error = ();
            fn try_from(response: $common_type) -> Result<Self, Self::Error> {
                if let $common_type::$command_name(cmd) = response {
                    Ok(cmd)
                } else {
                    Err(())
                }
            }
        }
    };
}

macro_rules! define_command {
    ($command_name:ident -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name;
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
    ($command_name:ident ($($param:ty),+) -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name($(pub $param,)+);
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
    ($command_name:ident {$($param_name:ident : $param_type:ty),+} -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name { $(pub $param_name: $param_type,)+}
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
}

macro_rules! define_common_types {
    ($($command_name:ident),+ $(,)?) => {
        #[derive(Serialize, Deserialize, Debug)]
        pub enum NodeCommand {
            $($command_name($command_name),)+
        }

        #[derive(Debug)]
        pub enum NodeCommandWithChannel {
            $($command_name(($command_name, CommandResponseChannel<$command_name>)),)+
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub enum NodeResponse {
            $($command_name(NodeCommandResponse<$command_name>),)+
        }

        impl NodeCommand {
            fn add_response_channel(self) -> (NodeCommandWithChannel,
        BoxFuture<'static, Result<NodeResponse, tokio::sync::oneshot::error::RecvError>>, // XXX
            ) {
                match self {
                    $(
                        NodeCommand::$command_name(cmd) => {
                            let (tx, rx) = oneshot::channel();
                            (
                                NodeCommandWithChannel::$command_name((cmd, tx)),
                                rx.map_ok(|r| NodeResponse::$command_name(
                                    NodeCommandResponse::<$command_name>(r)
                                )).boxed()
                            )
                        }
                    )+
                }
            }
        }
    };
}

define_common_types!(
    IsRunning,
    StartNode,
    GetLocalPeerId,
    GetSyncerInfo,
    GetPeerTrackerInfo,
    GetNetworkInfo,
    GetConnectedPeers,
    SetPeerTrust,
    WaitConnected,
    GetListeners,
    RequestHeader,
    RequestMultipleHeaders,
    GetHeader,
    GetMultipleHeaders,
    LastSeenNetworkHead,
    GetSamplingMetadata,
);

define_command!(IsRunning -> bool);
define_command!(StartNode(WasmNodeConfig) -> Result<()>);
define_command!(GetLocalPeerId -> String);
define_command!(GetSyncerInfo -> SyncingInfo);
define_command!(GetPeerTrackerInfo -> PeerTrackerInfo);
define_command!(GetNetworkInfo -> NetworkInfoSnapshot);
define_command!(GetConnectedPeers -> Vec<String>);
define_command!(SetPeerTrust { peer_id: String, is_trusted: bool } -> ());
define_command!(WaitConnected { trusted: bool } -> ());
define_command!(GetListeners -> Vec<Multiaddr>);
define_command!(RequestHeader(SingleHeaderQuery) -> Result<ExtendedHeader>);
define_command!(RequestMultipleHeaders(MultipleHeaderQuery) -> Vec<ExtendedHeader>);
define_command!(GetHeader(SingleHeaderQuery) -> ExtendedHeader);
define_command!(GetMultipleHeaders(MultipleHeaderQuery) -> Vec<ExtendedHeader>);
define_command!(LastSeenNetworkHead -> Option<ExtendedHeader>);
define_command!(GetSamplingMetadata { height: u64 } -> SamplingMetadata);

#[derive(Serialize, Deserialize, Debug)]
pub enum SingleHeaderQuery {
    Head,
    ByHash(Hash),
    ByHeight(u64),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MultipleHeaderQuery {
    GetVerified {
        from: ExtendedHeader,
        amount: u64,
    },
    Range {
        start_height: Option<u64>,
        end_height: Option<u64>,
    },
}

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
            // TODO: order
            NodeCommandWithChannel::IsRunning((_, response)) => {
                response.send(true).expect("channel_dropped")
            }
            NodeCommandWithChannel::StartNode((_, response)) => {
                response
                    .send(Err(WorkerError("already running".to_string())))
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
                let _ = self
                    .node
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
            NodeCommandWithChannel::RequestMultipleHeaders((command, response)) => {
                let headers = match command.0 {
                    MultipleHeaderQuery::GetVerified { from, amount } => self
                        .node
                        .request_verified_headers(&from, amount)
                        .await
                        .unwrap(),
                    MultipleHeaderQuery::Range { .. } => unreachable!("invalid command"),
                };

                response.send(headers).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetHeader((command, response)) => {
                let header = match command.0 {
                    SingleHeaderQuery::Head => self.node.get_local_head_header().await.unwrap(),
                    //SingleHeaderQuery::NetworkHead => self.node.get_network_head_header().unwrap(),
                    SingleHeaderQuery::ByHash(hash) => {
                        self.node.get_header_by_hash(&hash).await.ok().unwrap()
                    }
                    SingleHeaderQuery::ByHeight(height) => {
                        self.node.get_header_by_height(height).await.ok().unwrap()
                    }
                };
                response.send(header).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetMultipleHeaders((command, response)) => {
                let headers = match command.0 {
                    MultipleHeaderQuery::GetVerified { from, amount } => self
                        .node
                        .request_verified_headers(&from, amount)
                        .await
                        .unwrap(),
                    MultipleHeaderQuery::Range {
                        start_height,
                        end_height,
                    } => match (start_height, end_height) {
                        (None, None) => self.node.get_headers(..).await,
                        (Some(start), None) => self.node.get_headers(start..).await,
                        (None, Some(end)) => self.node.get_headers(..=end).await,
                        (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
                    }
                    .ok()
                    .unwrap(),
                };

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
    // make sure the callback doesn't get dropped
    _onconnect_callback: Closure<dyn Fn(MessageEvent)>,
    callbacks: Vec<Closure<dyn Fn(MessageEvent)>>,
    ports: Vec<MessagePort>,

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
                    let port: MessagePort = ev.ports().at(0).dyn_into().expect("invalid type");
                    local_tx
                        .send(WorkerMessage::NewConnection(port))
                        .await
                        .expect("send2 error");
                })
            });
        worker_scope.set_onconnect(Some(onconnect_callback.as_ref().unchecked_ref()));

        Self {
            _onconnect_callback: onconnect_callback,
            callbacks: Vec::new(),
            ports: Vec::new(),
            command_channel,
        }
    }

    fn add(&mut self, port: MessagePort) {
        debug_assert_eq!(self.callbacks.len(), self.ports.len());
        let client_id = self.callbacks.len();

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

                    local_tx
                        .send(WorkerMessage::Command(command_with_channel))
                        .await
                        .expect("send3 err");

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

        self.callbacks.push(client_message_callback);
        self.ports.push(port);

        info!("New connection: {client_id}");
    }

    fn respond_to(&self, client: ClientId, msg: NodeResponse) {
        let off = client.0;
        let v = to_value(&msg).expect("could not to_value");
        self.ports[off].post_message(&v).expect("err posttt");
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
                // XXX: properly
                let response = channel.await.expect("forwardding channel error");
                connector.respond_to(client_id, response);
            }
        }
    }

    warn!("EXIT EXIT EXIT");
}
