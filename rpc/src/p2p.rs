use jsonrpsee::proc_macros::rpc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AddrInfo {
    #[serde(rename = "ID")]
    pub id: String,
    // TODO: multiaddr
    #[serde(rename = "Addrs")]
    pub addrs: Vec<String>,
}

#[rpc(client)]
pub trait P2P {
    #[method(name = "p2p.Info")]
    async fn p2p_info(&self) -> Result<AddrInfo, Error>;
}
