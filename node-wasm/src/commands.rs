use std::fmt::Debug;

use celestia_types::nmt::Namespace;
use celestia_types::Blob;
use enum_as_inner::EnumAsInner;
use libp2p::Multiaddr;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::error;

use celestia_types::{hash::Hash, ExtendedHeader};
use lumina_node::node::{PeerTrackerInfo, SyncingInfo};
use lumina_node::store::SamplingMetadata;

use crate::client::WasmNodeConfig;
use crate::error::Error;
use crate::error::Result;
use crate::ports::MessagePortLike;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Command {
    Node(NodeCommand),
    Management(ManagementCommand),
    Subscribe(NodeSubscription),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ManagementCommand {
    InternalPing,
    GetEventsChannelName,
    ConnectPort(#[serde(skip)] Option<MessagePortLike>),
    IsRunning,
    StartNode(WasmNodeConfig),
    StopNode,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeSubscription {
    Headers {
        #[serde(skip)]
        port: Option<MessagePortLike>,
    },
    Blobs {
        #[serde(skip)]
        port: Option<MessagePortLike>,
        namespace: Namespace,
    },
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

impl Command {
    pub(crate) fn insert_port(&mut self, event_port: MessagePortLike) {
        match self {
            Command::Management(cmd) => match cmd {
                ManagementCommand::ConnectPort(port) => {
                    let _ = port.insert(event_port);
                }
                _ => (),
            },
            Command::Subscribe(sub) => match sub {
                NodeSubscription::Headers { port } => {
                    let _ = port.insert(event_port);
                }
                NodeSubscription::Blobs { port, .. } => {
                    let _ = port.insert(event_port);
                }
            },
            _ => (),
        };
    }

    pub(crate) fn take_port(&mut self) -> Option<MessagePortLike> {
        match self {
            Command::Management(cmd) => match cmd {
                ManagementCommand::ConnectPort(port) => port.take(),
                _ => None,
            },
            Command::Subscribe(sub) => match sub {
                NodeSubscription::Headers { port } => port.take(),
                NodeSubscription::Blobs { port, .. } => port.take(),
            },
            _ => None,
        }
    }
}
