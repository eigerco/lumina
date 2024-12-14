use libp2p::swarm::ConnectionCounters as Libp2pConnectionCounters;
use libp2p::swarm::NetworkInfo as Libp2pNetworkInfo;
use libp2p::PeerId as Libp2pPeerId;
use lumina_node::block_ranges::BlockRange as LuminaBlockRange;
use lumina_node::events::{NodeEvent as LuminaNodeEvent, NodeEventInfo as LuminaNodeEventInfo};
use lumina_node::node::SyncingInfo as LuminaSyncingInfo;
use std::str::FromStr;
use std::time::SystemTime;
use uniffi::Record;

/// Configuration options for the Lumina node
#[derive(Debug, Clone, Record)]
pub struct NodeStartConfig {
    /// Custom syncing window in seconds, defines maximum age of headers
    /// considered for syncing and sampling
    pub syncing_window_secs: Option<u32>,

    /// Custom pruning delay after the syncing window in seconds
    pub pruning_delay_secs: Option<u32>,

    /// Maximum number of headers in batch while syncing
    pub sync_batch_size: Option<u64>,

    /// Whether to listen for incoming connections
    pub enable_listener: bool,
}

impl Default for NodeStartConfig {
    fn default() -> Self {
        Self {
            syncing_window_secs: None,
            pruning_delay_secs: None,
            sync_batch_size: None,
            enable_listener: false,
        }
    }
}

#[derive(Record)]
pub struct NetworkInfo {
    /// The total number of connected peers.
    pub num_peers: u32,
    /// Counters of ongoing network connections.
    pub connection_counters: ConnectionCounters,
}

/// Counters of ongoing network connections.
#[derive(Record)]
pub struct ConnectionCounters {
    /// The current number of connections.
    pub num_connections: u32,
    /// The current number of pending connections.
    pub num_pending: u32,
    /// The current number of incoming connections.
    pub num_pending_incoming: u32,
    /// The current number of outgoing connections.
    pub num_pending_outgoing: u32,
    /// The current number of established connections.
    pub num_established: u32,
    /// The current number of established inbound connections.
    pub num_established_incoming: u32,
    /// The current number of established outbound connections.
    pub num_established_outgoing: u32,
}

impl From<Libp2pNetworkInfo> for NetworkInfo {
    fn from(info: Libp2pNetworkInfo) -> Self {
        Self {
            num_peers: info.num_peers() as u32,
            connection_counters: info.connection_counters().into(),
        }
    }
}

impl From<&Libp2pConnectionCounters> for ConnectionCounters {
    fn from(counters: &Libp2pConnectionCounters) -> Self {
        Self {
            num_connections: counters.num_connections(),
            num_pending: counters.num_pending(),
            num_pending_incoming: counters.num_pending_incoming(),
            num_pending_outgoing: counters.num_pending_outgoing(),
            num_established: counters.num_established(),
            num_established_incoming: counters.num_established_incoming(),
            num_established_outgoing: counters.num_established_outgoing(),
        }
    }
}

/// A range of blocks.
#[derive(Record)]
pub struct BlockRange {
    pub start: u64,
    pub end: u64,
}

impl From<LuminaBlockRange> for BlockRange {
    fn from(range: LuminaBlockRange) -> Self {
        Self {
            start: *range.start(),
            end: *range.end(),
        }
    }
}

/// Status of the node syncing.
#[derive(Record)]
pub struct SyncingInfo {
    /// Ranges of headers that are already synchronised
    pub stored_headers: Vec<BlockRange>,
    /// Syncing target. The latest height seen in the network that was successfully verified.
    pub subjective_head: u64,
}

impl From<LuminaSyncingInfo> for SyncingInfo {
    fn from(info: LuminaSyncingInfo) -> Self {
        Self {
            stored_headers: info
                .stored_headers
                .into_inner()
                .into_iter()
                .map(BlockRange::from)
                .collect(),
            subjective_head: info.subjective_head,
        }
    }
}

#[derive(Record, Clone, Debug)]
pub struct PeerId {
    /// The peer ID stored as base58 string.
    pub peer_id: String,
}

impl PeerId {
    pub fn to_libp2p(&self) -> Result<Libp2pPeerId, String> {
        Libp2pPeerId::from_str(&self.peer_id).map_err(|e| format!("Invalid peer ID format: {}", e))
    }

    pub fn from_libp2p(peer_id: &Libp2pPeerId) -> Self {
        Self {
            peer_id: peer_id.to_string(),
        }
    }
}

impl From<Libp2pPeerId> for PeerId {
    fn from(peer_id: Libp2pPeerId) -> Self {
        Self {
            peer_id: peer_id.to_string(),
        }
    }
}

#[derive(Record)]
pub struct ShareCoordinate {
    pub row: u16,
    pub column: u16,
}

/// Events emitted by the node.
#[derive(uniffi::Enum)]
pub enum NodeEvent {
    /// Node is connecting to bootnodes
    ConnectingToBootnodes,
    /// Peer just connected
    PeerConnected {
        /// The ID of the peer.
        id: PeerId,
        /// Whether peer was in the trusted list or not.
        trusted: bool,
    },
    PeerDisconnected {
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
        shares: Vec<ShareCoordinate>,
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
        /// How much time sampling took in milliseconds.
        took_ms: u64,
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
        /// How much time fetching took in milliseconds.
        took_ms: u64,
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
        /// How much time fetching took in milliseconds.
        took_ms: u64,
    },
    /// Fetching headers of a specific block range just failed.
    FetchingHeadersFailed {
        /// Start of the range.
        from_height: u64,
        /// End of the range (included).
        to_height: u64,
        /// A human readable error.
        error: String,
        /// How much time fetching took in milliseconds.
        took_ms: u64,
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

impl From<LuminaNodeEvent> for NodeEvent {
    fn from(event: LuminaNodeEvent) -> Self {
        match event {
            LuminaNodeEvent::ConnectingToBootnodes => NodeEvent::ConnectingToBootnodes,
            LuminaNodeEvent::PeerConnected { id, trusted } => NodeEvent::PeerConnected {
                id: PeerId::from_libp2p(&id),
                trusted,
            },
            LuminaNodeEvent::PeerDisconnected { id, trusted } => NodeEvent::PeerDisconnected {
                id: PeerId::from_libp2p(&id),
                trusted,
            },
            LuminaNodeEvent::SamplingStarted {
                height,
                square_width,
                shares,
            } => NodeEvent::SamplingStarted {
                height,
                square_width,
                shares: shares
                    .into_iter()
                    .map(|(row, col)| ShareCoordinate { row, column: col })
                    .collect(),
            },
            LuminaNodeEvent::ShareSamplingResult {
                height,
                square_width,
                row,
                column,
                accepted,
            } => NodeEvent::ShareSamplingResult {
                height,
                square_width,
                row,
                column,
                accepted,
            },
            LuminaNodeEvent::SamplingFinished {
                height,
                accepted,
                took,
            } => NodeEvent::SamplingFinished {
                height,
                accepted,
                took_ms: took.as_millis() as u64,
            },
            LuminaNodeEvent::FatalDaserError { error } => NodeEvent::FatalDaserError { error },
            LuminaNodeEvent::AddedHeaderFromHeaderSub { height } => {
                NodeEvent::AddedHeaderFromHeaderSub { height }
            }
            LuminaNodeEvent::FetchingHeadHeaderStarted => NodeEvent::FetchingHeadHeaderStarted,
            LuminaNodeEvent::FetchingHeadHeaderFinished { height, took } => {
                NodeEvent::FetchingHeadHeaderFinished {
                    height,
                    took_ms: took.as_millis() as u64,
                }
            }
            LuminaNodeEvent::FetchingHeadersStarted {
                from_height,
                to_height,
            } => NodeEvent::FetchingHeadersStarted {
                from_height,
                to_height,
            },
            LuminaNodeEvent::FetchingHeadersFinished {
                from_height,
                to_height,
                took,
            } => NodeEvent::FetchingHeadersFinished {
                from_height,
                to_height,
                took_ms: took.as_millis() as u64,
            },
            LuminaNodeEvent::FetchingHeadersFailed {
                from_height,
                to_height,
                error,
                took,
            } => NodeEvent::FetchingHeadersFailed {
                from_height,
                to_height,
                error,
                took_ms: took.as_millis() as u64,
            },
            LuminaNodeEvent::FatalSyncerError { error } => NodeEvent::FatalSyncerError { error },
            LuminaNodeEvent::PrunedHeaders { to_height } => NodeEvent::PrunedHeaders { to_height },
            LuminaNodeEvent::FatalPrunerError { error } => NodeEvent::FatalPrunerError { error },
            LuminaNodeEvent::NetworkCompromised => NodeEvent::NetworkCompromised,
            LuminaNodeEvent::NodeStopped => NodeEvent::NodeStopped,
            _ => panic!("Unknown event: {:?}", event),
        }
    }
}

/// Information about a node event.
#[derive(Record)]
pub struct NodeEventInfo {
    /// The event that occurred.
    pub event: NodeEvent,
    /// Unix timestamp in milliseconds when the event occurred.
    pub timestamp: u64,
    /// Source file path where the event was emitted.
    pub file_path: String,
    /// Line number in source file where event was emitted.
    pub file_line: u32,
}

impl From<LuminaNodeEventInfo> for NodeEventInfo {
    fn from(info: LuminaNodeEventInfo) -> Self {
        Self {
            event: info.event.into(),
            timestamp: info
                .time
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            file_path: info.file_path.to_string(),
            file_line: info.file_line,
        }
    }
}
