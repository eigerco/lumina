//! Events generated by `Node`

use std::fmt;
use std::panic::Location;
use std::time::Duration;

use libp2p::PeerId;
use lumina_utils::time::SystemTime;
use serde::Serialize;
use tokio::sync::broadcast;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// An error returned from the [`EventSubscriber::recv`].
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// Node and all its event senders are closed.
    #[error("Event channel closed")]
    Closed,
}

/// An error returned from the [`EventSubscriber::try_recv`].
#[derive(Debug, thiserror::Error)]
pub enum TryRecvError {
    /// The event channel is currently empty.
    #[error("Event channel empty")]
    Empty,
    /// Node and all its event senders are closed.
    #[error("Event channel closed")]
    Closed,
}

/// A channel which users can subscribe for events.
#[derive(Debug)]
pub(crate) struct EventChannel {
    tx: broadcast::Sender<NodeEventInfo>,
}

/// `EventPublisher` is used to broadcast events generated by [`Node`] to [`EventSubscriber`]s.
///
/// [`Node`]: crate::node::Node
#[derive(Debug, Clone)]
pub(crate) struct EventPublisher {
    tx: broadcast::Sender<NodeEventInfo>,
}

/// `EventSubscriber` can be used by users to receive events from [`Node`].
///
/// [`Node`]: crate::node::Node
#[derive(Debug)]
pub struct EventSubscriber {
    rx: broadcast::Receiver<NodeEventInfo>,
}

impl EventChannel {
    /// Create a new `EventChannel`.
    pub(crate) fn new() -> EventChannel {
        let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        EventChannel { tx }
    }

    /// Creates a new [`EventPublisher`].
    pub(crate) fn publisher(&self) -> EventPublisher {
        EventPublisher {
            tx: self.tx.clone(),
        }
    }

    /// Creates a new [`EventSubscriber`].
    pub(crate) fn subscribe(&self) -> EventSubscriber {
        EventSubscriber {
            rx: self.tx.subscribe(),
        }
    }
}

impl Default for EventChannel {
    fn default() -> Self {
        EventChannel::new()
    }
}

impl EventPublisher {
    pub(crate) fn send(&self, event: NodeEvent) {
        let time = SystemTime::now();
        let location: &'static Location<'static> = Location::caller();

        // Error is produced if there aren't any subscribers. Since this is
        // a valid case, we ignore the error.
        let _ = self.tx.send(NodeEventInfo {
            event,
            time,
            file_path: location.file(),
            file_line: location.line(),
        });
    }
}

impl EventSubscriber {
    /// Receive an event from [`Node`].
    ///
    /// # Cancel safety
    ///
    /// This method is cancel-safe.
    ///
    /// [`Node`]: crate::node::Node
    pub async fn recv(&mut self) -> Result<NodeEventInfo, RecvError> {
        loop {
            match self.rx.recv().await {
                Ok(val) => return Ok(val),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            }
        }
    }

    /// Attempts to receive an already queued event from [`Node`] without awaiting.
    ///
    /// If no events are queued, `Err(TryRecvError::Empty)` is returned.
    ///
    /// [`Node`]: crate::node::Node
    pub fn try_recv(&mut self) -> Result<NodeEventInfo, TryRecvError> {
        loop {
            match self.rx.try_recv() {
                Ok(val) => return Ok(val),
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Slow consumer. We will receive a message on the next call.
                    continue;
                }
                Err(broadcast::error::TryRecvError::Empty) => return Err(TryRecvError::Empty),
                Err(broadcast::error::TryRecvError::Closed) => return Err(TryRecvError::Closed),
            }
        }
    }
}

/// This struct include the [`NodeEvent`] and some extra information about the event.
#[derive(Debug, Clone, Serialize)]
pub struct NodeEventInfo {
    /// The event
    pub event: NodeEvent,
    #[cfg_attr(
        target_arch = "wasm32",
        serde(serialize_with = "serialize_system_time")
    )]
    /// When the event was generated.
    pub time: SystemTime,
    /// Which file generated the event.
    pub file_path: &'static str,
    /// Which line in the file generated the event.
    pub file_line: u32,
}

/// The events that [`Node`] can generate.
///
/// [`Node`]: crate::node::Node
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NodeEvent {
    /// Node is connecting to bootnodes
    ConnectingToBootnodes,

    /// Peer just connected
    PeerConnected {
        #[serde(serialize_with = "serialize_as_string")]
        /// The ID of the peer.
        id: PeerId,
        /// Whether peer was in the trusted list or not.
        trusted: bool,
    },

    /// Peer just disconnected
    PeerDisconnected {
        #[serde(serialize_with = "serialize_as_string")]
        /// The ID of the peer.
        id: PeerId,
        /// Whether peer was in the trusted list or not.
        trusted: bool,
    },

    /// Sampling just started.
    SamplingStarted {
        /// The block height that will be sampled.
        height: u64,
        /// The square width of the block.
        square_width: u16,
        /// The coordinates of the shares that will be sampled.
        shares: Vec<(u16, u16)>,
    },

    /// A share was sampled.
    ShareSamplingResult {
        /// The block height of the share.
        height: u64,
        /// The square width of the block.
        square_width: u16,
        /// The row of the share.
        row: u16,
        /// The column of the share.
        column: u16,
        /// The result of the sampling of the share.
        accepted: bool,
    },

    /// Sampling just finished.
    SamplingFinished {
        /// The block height that was sampled.
        height: u64,
        /// The overall result of the sampling.
        accepted: bool,
        /// How much time sampling took.
        took: Duration,
    },

    /// Data sampling fatal error.
    FatalDaserError {
        /// A human readable error.
        error: String,
    },

    /// A new header was added from HeaderSub.
    AddedHeaderFromHeaderSub {
        /// The height of the header.
        height: u64,
    },

    /// Fetching header of network head just started.
    FetchingHeadHeaderStarted,

    /// Fetching header of network head just finished.
    FetchingHeadHeaderFinished {
        /// The height of the network head.
        height: u64,
        /// How much time fetching took.
        took: Duration,
    },

    /// Fetching headers of a specific block range just started.
    FetchingHeadersStarted {
        /// Start of the range.
        from_height: u64,
        /// End of the range (included).
        to_height: u64,
    },

    /// Fetching headers of a specific block range just finished.
    FetchingHeadersFinished {
        /// Start of the range.
        from_height: u64,
        /// End of the range (included).
        to_height: u64,
        /// How much time fetching took.
        took: Duration,
    },

    /// Fetching headers of a specific block range just failed.
    FetchingHeadersFailed {
        /// Start of the range.
        from_height: u64,
        /// End of the range (included).
        to_height: u64,
        /// A human readable error.
        error: String,
        /// How much time fetching took.
        took: Duration,
    },

    /// Header syncing fatal error.
    FatalSyncerError {
        /// A human readable error.
        error: String,
    },

    /// Pruned headers up to and including specified height.
    PrunedHeaders {
        /// Last header height that was pruned
        to_height: u64,
    },

    /// Pruning fatal error.
    FatalPrunerError {
        /// A human readable error.
        error: String,
    },

    /// Network was compromised.
    ///
    /// This happens when a valid bad encoding fraud proof is received.
    /// Ideally it would never happen, but protection needs to exist.
    /// In case of compromised network, syncing and data sampling will
    /// stop immediately.
    NetworkCompromised,

    /// Node stopped.
    NodeStopped,
}

impl NodeEvent {
    /// Returns `true` if the event indicates an error.
    pub fn is_error(&self) -> bool {
        match self {
            NodeEvent::FatalDaserError { .. }
            | NodeEvent::FatalSyncerError { .. }
            | NodeEvent::FatalPrunerError { .. }
            | NodeEvent::FetchingHeadersFailed { .. }
            | NodeEvent::NetworkCompromised => true,
            NodeEvent::ConnectingToBootnodes
            | NodeEvent::PeerConnected { .. }
            | NodeEvent::PeerDisconnected { .. }
            | NodeEvent::SamplingStarted { .. }
            | NodeEvent::ShareSamplingResult { .. }
            | NodeEvent::SamplingFinished { .. }
            | NodeEvent::AddedHeaderFromHeaderSub { .. }
            | NodeEvent::FetchingHeadHeaderStarted
            | NodeEvent::FetchingHeadHeaderFinished { .. }
            | NodeEvent::FetchingHeadersStarted { .. }
            | NodeEvent::FetchingHeadersFinished { .. }
            | NodeEvent::PrunedHeaders { .. }
            | NodeEvent::NodeStopped => false,
        }
    }
}

impl fmt::Display for NodeEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeEvent::ConnectingToBootnodes => {
                write!(f, "Connecting to bootnodes")
            }
            NodeEvent::PeerConnected { id, trusted } => {
                if *trusted {
                    write!(f, "Trusted peer connected: {id}")
                } else {
                    write!(f, "Peer connected: {id}")
                }
            }
            NodeEvent::PeerDisconnected { id, trusted } => {
                if *trusted {
                    write!(f, "Trusted peer disconnected: {id}")
                } else {
                    write!(f, "Peer disconnected: {id}")
                }
            }
            NodeEvent::SamplingStarted {
                height,
                square_width,
                shares,
            } => {
                write!(f, "Sampling of block {height} started. Square: {square_width}x{square_width}, Shares: {shares:?}")
            }
            NodeEvent::ShareSamplingResult {
                height,
                row,
                column,
                accepted,
                ..
            } => {
                let acc = if *accepted { "accepted" } else { "rejected" };
                write!(
                    f,
                    "Sampling for share [{row}, {column}] of block {height} was {acc}"
                )
            }
            NodeEvent::SamplingFinished { height, took, .. } => {
                write!(f, "Sampling of block {height} finished. Took: {took:?}")
            }
            NodeEvent::FatalDaserError { error } => {
                write!(f, "Daser stopped because of a fatal error: {error}")
            }
            NodeEvent::AddedHeaderFromHeaderSub { height } => {
                write!(f, "Added header {height} from header-sub")
            }
            NodeEvent::FetchingHeadHeaderStarted => {
                write!(f, "Fetching header of network head block started")
            }
            NodeEvent::FetchingHeadHeaderFinished { height, took } => {
                write!(f, "Fetching header of network head block finished. Height: {height}, Took: {took:?}")
            }
            NodeEvent::FetchingHeadersStarted {
                from_height,
                to_height,
            } => {
                if from_height == to_height {
                    write!(f, "Fetching header of block {from_height} started")
                } else {
                    write!(
                        f,
                        "Fetching headers of blocks {from_height}-{to_height} started"
                    )
                }
            }
            NodeEvent::FetchingHeadersFinished {
                from_height,
                to_height,
                took,
            } => {
                if from_height == to_height {
                    write!(
                        f,
                        "Fetching header of block {from_height} finished. Took: {took:?}"
                    )
                } else {
                    write!(f, "Fetching headers of blocks {from_height}-{to_height} finished. Took: {took:?}")
                }
            }
            NodeEvent::FetchingHeadersFailed {
                from_height,
                to_height,
                error,
                took,
            } => {
                if from_height == to_height {
                    write!(
                        f,
                        "Fetching header of block {from_height} failed. Took: {took:?}, Error: {error}"
                    )
                } else {
                    write!(f, "Fetching headers of blocks {from_height}-{to_height} failed. Took: {took:?}, Error: {error}")
                }
            }
            NodeEvent::FatalSyncerError { error } => {
                write!(f, "Syncer stopped because of a fatal error: {error}")
            }
            Self::PrunedHeaders { to_height } => {
                write!(f, "Pruned headers up to and including {to_height}")
            }
            NodeEvent::FatalPrunerError { error } => {
                write!(f, "Pruner stopped because of a fatal error: {error}")
            }
            NodeEvent::NetworkCompromised => {
                write!(f, "The network is compromised and should not be trusted. ")?;
                write!(f, "Node stopped synchronizing and sampling, but you can still make some queries to the network.")
            }
            NodeEvent::NodeStopped => {
                write!(f, "Node stopped")
            }
        }
    }
}

fn serialize_as_string<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: ToString,
    S: serde::ser::Serializer,
{
    value.to_string().serialize(serializer)
}

#[cfg(target_arch = "wasm32")]
fn serialize_system_time<S>(value: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    // Javascript expresses time as f64 and in milliseconds.
    let js_time = value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime is before 1970")
        .as_secs_f64()
        * 1000.0;
    js_time.serialize(serializer)
}
