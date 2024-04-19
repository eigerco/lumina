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

#[wasm_bindgen]
pub struct NetworkInfo(SwarmNetworkInfo);

impl From<SwarmNetworkInfo> for NetworkInfo {
    fn from(info: SwarmNetworkInfo) -> NetworkInfo {
        NetworkInfo(info)
    }
}

#[wasm_bindgen]
impl NetworkInfo {
    #[wasm_bindgen(getter)]
    pub fn num_peers(&self) -> usize {
        self.0.num_peers()
    }

    pub fn connection_counters(&self) -> ConnectionCounters {
        self.0.connection_counters().clone().into()
    }
}

#[wasm_bindgen]
pub struct ConnectionCounters(SwarmConnectionCounters);

impl From<SwarmConnectionCounters> for ConnectionCounters {
    fn from(info: SwarmConnectionCounters) -> ConnectionCounters {
        ConnectionCounters(info)
    }
}

#[wasm_bindgen]
impl ConnectionCounters {
    #[wasm_bindgen(getter)]
    pub fn num_connections(&self) -> u32 {
        self.0.num_connections()
    }

    #[wasm_bindgen(getter)]
    pub fn num_pending(&self) -> u32 {
        self.0.num_pending()
    }

    #[wasm_bindgen(getter)]
    pub fn num_pending_incoming(&self) -> u32 {
        self.0.num_pending_incoming()
    }

    #[wasm_bindgen(getter)]
    pub fn num_pending_outgoing(&self) -> u32 {
        self.0.num_pending_outgoing()
    }

    #[wasm_bindgen(getter)]
    pub fn num_established_incoming(&self) -> u32 {
        self.0.num_established_incoming()
    }

    #[wasm_bindgen(getter)]
    pub fn num_established_outgoing(&self) -> u32 {
        self.0.num_established_outgoing()
    }

    #[wasm_bindgen(getter)]
    pub fn num_established(&self) -> u32 {
        self.0.num_established()
    }
}
