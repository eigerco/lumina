use libp2p::swarm::{
    ConnectionCounters as SwarmConnectionCounters, NetworkInfo as SwarmNetworkInfo,
};
use wasm_bindgen::prelude::*;

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
