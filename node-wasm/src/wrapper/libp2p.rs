use libp2p::swarm::{
    ConnectionCounters as SwarmConnectionCounters, NetworkInfo as SwarmNetworkInfo,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct NetworkInfoSnapshot {
    pub num_peers: usize,
    connection_counters: ConnectionCountersSnapshot,
}

#[wasm_bindgen]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ConnectionCountersSnapshot {
    pub num_connections: u32,
    pub num_pending: u32,
    pub num_pending_incoming: u32,
    pub num_pending_outgoing: u32,
    pub num_established_incoming: u32,
    pub num_established_outgoing: u32,
    pub num_established: u32,
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
            num_established_incoming: counters.num_established_incoming(),
            num_established_outgoing: counters.num_established_outgoing(),
            num_established: counters.num_established(),
        }
    }
}
