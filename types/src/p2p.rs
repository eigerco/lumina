use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AddrInfo {
    #[serde(rename = "ID")]
    pub id: PeerId,
    pub addrs: Vec<Multiaddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourceManagerStats {
    pub system: Stat,
    pub transient: Stat,
    pub services: HashMap<String, Stat>,
    pub protocols: HashMap<String, Stat>,
    pub peers: HashMap<String, Stat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerId(
    #[serde(with = "tendermint_proto::serializers::from_str")] pub libp2p_identity::PeerId,
);

impl From<libp2p_identity::PeerId> for PeerId {
    fn from(value: libp2p_identity::PeerId) -> Self {
        PeerId(value)
    }
}

impl From<PeerId> for libp2p_identity::PeerId {
    fn from(value: PeerId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BandwidthStats {
    pub total_in: f32,
    pub total_out: f32,
    pub rate_in: f32,
    pub rate_out: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Connectedness {
    NotConnected = 0,
    Connected = 1,
    CanConnect = 2,
    CannotConnect = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Reachability {
    Unknown = 0,
    Public = 1,
    Private = 2,
}
