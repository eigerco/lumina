use std::fmt::Debug;

use enum_as_inner::EnumAsInner;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::error;

use celestia_types::hash::Hash;
use celestia_types::nmt::Namespace;
use celestia_types::{Blob, ExtendedHeader};
use lumina_node::node::{PeerTrackerInfo, SyncingInfo};
use lumina_node::store::SamplingMetadata;

use crate::client::WasmNodeConfig;
use crate::error::{Error, Result};
use crate::ports::MessagePortLike;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Command {
    Node(NodeCommand),
    Management(WorkerCommand),
    Subscribe(SubscriptionCommand),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum WorkerCommand {
    InternalPing,
    GetEventsChannelName,
    ConnectPort(#[serde(skip)] Option<MessagePortLike>),
    IsRunning,
    StartNode(WasmNodeConfig),
    StopNode,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum SubscriptionCommand {
    Headers,
    Blobs(Namespace),
    Shares(Namespace),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeCommand {
    GetLocalPeerId,
    GetSyncerInfo,
    GetPeerTrackerInfo,
    GetNetworkInfo,
    GetConnectedPeers,
    SetPeerTrust {
        peer_id: PeerId,
        is_trusted: bool,
    },
    WaitConnected {
        trusted: bool,
    },
    GetListeners,
    RequestHeader(SingleHeaderQuery),
    GetVerifiedHeaders {
        from: ExtendedHeader,
        amount: u64,
    },
    GetHeadersRange {
        start_height: Option<u64>,
        end_height: Option<u64>,
    },
    GetHeader(SingleHeaderQuery),
    LastSeenNetworkHead,
    GetSamplingMetadata {
        height: u64,
    },
    RequestAllBlobs {
        namespace: Namespace,
        block_height: u64,
        timeout_secs: Option<f64>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum SingleHeaderQuery {
    Head,
    ByHash(Hash),
    ByHeight(u64),
}

pub(crate) type WorkerResult = Result<WorkerResponse, WorkerError>;

#[derive(Serialize, Deserialize, Debug, EnumAsInner)]
pub(crate) enum WorkerResponse {
    Ok,
    InternalPong,
    Subscribed(#[serde(skip)] Option<MessagePortLike>),
    PortConnected(bool),
    IsRunning(bool),
    EventsChannelName(String),
    LocalPeerId(String),
    SyncerInfo(SyncingInfo),
    PeerTrackerInfo(PeerTrackerInfo),
    NetworkInfo(NetworkInfoSnapshot),
    ConnectedPeers(Vec<String>),
    Listeners(Vec<Multiaddr>),
    Header(ExtendedHeader),
    Headers(Vec<ExtendedHeader>),
    LastSeenNetworkHead(Option<ExtendedHeader>),
    SamplingMetadata(Option<SamplingMetadata>),
    Blobs(Vec<Blob>),
}

#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
pub(crate) enum WorkerError {
    /// Node is already running
    #[error("Node already running")]
    NodeAlreadyRunning,
    /// Node is not running
    #[error("Node not running")]
    NodeNotRunning,
    /// Empty response received, dropped responder?
    #[error("Empty response")]
    EmptyResponse,
    /// Worker received unrecognised command
    #[error("invalid command received")]
    InvalidCommandReceived,
    /// Received invalid response type
    #[error("invalid response type")]
    InvalidResponseType,
    /// Error forwarded from Node
    #[error("Node error: {0}")]
    Node(String),
}

impl From<Error> for WorkerError {
    fn from(error: Error) -> Self {
        WorkerError::Node(error.to_string())
    }
}

pub(crate) struct CommandWithResponder {
    pub command: Command,
    pub responder: oneshot::Sender<WorkerResult>,
}

pub(crate) trait HasMessagePort {
    fn take_port(&mut self) -> Option<MessagePortLike>;
}

impl HasMessagePort for Command {
    fn take_port(&mut self) -> Option<MessagePortLike> {
        if let Command::Management(WorkerCommand::ConnectPort(maybe_port)) = self {
            return maybe_port.take();
        }
        None
    }
}

impl HasMessagePort for WorkerResult {
    fn take_port(&mut self) -> Option<MessagePortLike> {
        if let Ok(WorkerResponse::Subscribed(maybe_port)) = self {
            return maybe_port.take();
        }
        None
    }
}
