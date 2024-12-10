use libp2p::swarm::ConnectionCounters as Libp2pConnectionCounters;
use libp2p::swarm::NetworkInfo as Libp2pNetworkInfo;
use libp2p::PeerId as Libp2pPeerId;
use lumina_node::block_ranges::BlockRange as LuminaBlockRange;
use lumina_node::events::{NodeEvent as LuminaNodeEvent, NodeEventInfo as LuminaNodeEventInfo};
use lumina_node::network;
use lumina_node::node::PeerTrackerInfo as LuminaPeerTrackerInfo;
use lumina_node::node::SyncingInfo as LuminaSyncingInfo;
use std::str::FromStr;
use std::time::SystemTime;
use uniffi::{Enum, Record};

/// Supported Celestia networks.
#[derive(Debug, Default, Clone, Copy, Enum)]
pub enum Network {
    /// Celestia mainnet.
    #[default]
    Mainnet,
    /// Arabica testnet.
    Arabica,
    /// Mocha testnet.
    Mocha,
    /// Private local network.
    Private,
}

// From implementation for converting between Lumina and Uniffi types
impl From<Network> for network::Network {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => network::Network::Mainnet,
            Network::Arabica => network::Network::Arabica,
            Network::Mocha => network::Network::Mocha,
            Network::Private => network::Network::Private,
        }
    }
}

/// Statistics of the connected peers
#[derive(Debug, Clone, Default, Record)]
pub struct PeerTrackerInfo {
    /// Number of the connected peers.
    pub num_connected_peers: u64,
    /// Number of the connected trusted peers.
    pub num_connected_trusted_peers: u64,
}

impl From<LuminaPeerTrackerInfo> for PeerTrackerInfo {
    fn from(info: LuminaPeerTrackerInfo) -> Self {
        Self {
            num_connected_peers: info.num_connected_peers,
            num_connected_trusted_peers: info.num_connected_trusted_peers,
        }
    }
}

#[derive(Record)]
pub struct NetworkInfo {
    pub num_peers: u32,
    pub connection_counters: ConnectionCounters,
}

#[derive(Record)]
pub struct ConnectionCounters {
    pub num_connections: u32,
    pub num_pending: u32,
    pub num_pending_incoming: u32,
    pub num_pending_outgoing: u32,
    pub num_established: u32,
    pub num_established_incoming: u32,
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

#[derive(Record)]
pub struct SyncingInfo {
    pub stored_headers: Vec<BlockRange>,
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
    // Store as base58 string for now
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

#[derive(uniffi::Enum)]
pub enum NodeEvent {
    ConnectingToBootnodes,
    PeerConnected {
        id: PeerId,
        trusted: bool,
    },
    PeerDisconnected {
        id: PeerId,
        trusted: bool,
    },
    SamplingStarted {
        height: u64,
        square_width: u16,
        shares: Vec<ShareCoordinate>,
    },
    ShareSamplingResult {
        height: u64,
        square_width: u16,
        row: u16,
        column: u16,
        accepted: bool,
    },
    SamplingFinished {
        height: u64,
        accepted: bool,
        took_ms: u64,
    },
    FatalDaserError {
        error: String,
    },
    AddedHeaderFromHeaderSub {
        height: u64,
    },
    FetchingHeadHeaderStarted,
    FetchingHeadHeaderFinished {
        height: u64,
        took_ms: u64,
    },
    FetchingHeadersStarted {
        from_height: u64,
        to_height: u64,
    },
    FetchingHeadersFinished {
        from_height: u64,
        to_height: u64,
        took_ms: u64,
    },
    FetchingHeadersFailed {
        from_height: u64,
        to_height: u64,
        error: String,
        took_ms: u64,
    },
    FatalSyncerError {
        error: String,
    },
    PrunedHeaders {
        to_height: u64,
    },
    FatalPrunerError {
        error: String,
    },
    NetworkCompromised,
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

#[derive(Record)]
pub struct NodeEventInfo {
    pub event: NodeEvent,
    pub timestamp: u64, // Unix timestamp in milliseconds for now
    pub file_path: String,
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
