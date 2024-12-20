use libp2p::swarm::ConnectionCounters as Libp2pConnectionCounters;
use libp2p::swarm::NetworkInfo as Libp2pNetworkInfo;
use uniffi::Record;

#[derive(Record)]
pub struct NetworkInfo {
    /// The total number of connected peers.
    pub num_peers: u32,
    /// Counters of ongoing network connections.
    connection_counters: ConnectionCounters,
}

/// Counters of ongoing network connections.
#[derive(Record)]
struct ConnectionCounters {
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
