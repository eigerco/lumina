use libp2p::swarm::{
    ConnectionCounters as SwarmConnectionCounters, NetworkInfo as SwarmNetworkInfo,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inspectable)]
#[derive(Debug, Serialize, Deserialize)]
/// Information about the connections
pub(crate) struct NetworkInfoSnapshot {
    /// The number of connected peers, i.e. peers with whom at least one established connection exists.
    pub num_peers: usize,
    /// Gets counters for ongoing network connections.
    pub connection_counters: ConnectionCountersSnapshot,
}

#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ConnectionCountersSnapshot {
    /// The total number of connections, both pending and established.
    pub num_connections: u32,
    /// The total number of pending connections, both incoming and outgoing.
    pub num_pending: u32,
    /// The total number of pending connections, both incoming and outgoing.
    pub num_pending_incoming: u32,
    /// The number of outgoing connections being established.
    pub num_pending_outgoing: u32,
    /// The number of outgoing connections being established.
    pub num_established: u32,
    /// The number of established incoming connections.
    pub num_established_incoming: u32,
    /// The number of established outgoing connections.
    pub num_established_outgoing: u32,
}
impl From<SwarmNetworkInfo> for NetworkInfoSnapshot {
    fn from(info: SwarmNetworkInfo) -> Self {
        Self {
            num_peers: info.num_peers(),
            connection_counters: ConnectionCountersSnapshot::from(info.connection_counters()),
        }
    }
}

impl From<&SwarmConnectionCounters> for ConnectionCountersSnapshot {
    fn from(counters: &SwarmConnectionCounters) -> Self {
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
