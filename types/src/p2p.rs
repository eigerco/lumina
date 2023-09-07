use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use serde_repr::Deserialize_repr;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct AddrInfo {
    #[serde(rename = "ID")]
    pub id: PeerId,
    #[serde(rename = "Addrs")]
    pub addrs: Vec<Multiaddr>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Stat {
    pub num_streams_inbound: u32,
    pub num_streams_outbound: u32,
    pub num_conns_inbound: u32,
    pub num_conns_outbound: u32,
    #[serde(rename = "NumFD")]
    pub num_fd: u32,
    pub memory: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourceManagerStats {
    pub system: Stat,
    pub transient: Stat,
    pub services: HashMap<String, Stat>,
    pub protocols: HashMap<String, Stat>,
    pub peers: HashMap<String, Stat>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerId(#[serde(with = "tendermint_proto::serializers::from_str")] pub libp2p::PeerId);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BandwidthStats {
    pub total_in: f32,
    pub total_out: f32,
    pub rate_in: f32,
    pub rate_out: f32,
}

#[derive(Debug, PartialEq, Eq, Deserialize_repr)]
#[repr(u8)]
pub enum Connectedness {
    NotConnected = 0,
    Connected = 1,
    CanConnect = 2,
    CannotConnect = 3,
}

#[derive(Debug, PartialEq, Eq, Deserialize_repr)]
#[repr(u8)]
pub enum Reachability {
    Unknown = 0,
    Public = 1,
    Private = 2,
}
