use jsonrpsee::proc_macros::rpc;
use libp2p::Multiaddr;
use serde::{de, de::Error, ser, Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
pub struct AddrInfo {
    #[serde(rename = "ID")]
    pub id: PeerId,
    #[serde(rename = "Addrs")]
    pub addrs: Vec<Multiaddr>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stat {
    #[serde(rename = "NumStreamsInbound")]
    pub streams_inbound: u32,
    #[serde(rename = "NumStreamsOutbound")]
    pub streams_outbound: u32,
    #[serde(rename = "NumConnsInbound")]
    pub connections_inbound: u32,
    #[serde(rename = "NumConnsOutbound")]
    pub connections_outbound: u32,
    #[serde(rename = "NumFD")]
    pub file_descriptors: u32,
    #[serde(rename = "Memory")]
    pub memory: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResourceManagerStats {
    #[serde(rename = "System")]
    pub system: Stat,
    #[serde(rename = "Transient")]
    pub transient: Stat,
    #[serde(rename = "Services")]
    pub services: HashMap<String, Stat>,
    #[serde(rename = "Protocols")]
    pub protocols: HashMap<String, Stat>,
    #[serde(rename = "Peers")]
    pub peers: HashMap<String, Stat>,
}

#[derive(Debug, Error)]
pub enum PeerIdError {
    #[error("unable to decode base58 string")]
    Base58Error,
    #[error("libp2p error")]
    Libp2pError(#[from] libp2p_identity::ParseError),
}

#[derive(Debug, PartialEq, Eq)]
pub struct PeerId(pub libp2p::PeerId);

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<PeerId, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }
}

impl Serialize for PeerId {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        s.serialize_str(&self.0.to_base58())
    }
}

impl FromStr for PeerId {
    type Err = PeerIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| PeerIdError::Base58Error)?;

        Ok(Self(libp2p::PeerId::from_bytes(&bytes)?))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BandwidthStats {
    #[serde(rename = "TotalIn")]
    pub total_in: f32,

    #[serde(rename = "TotalOut")]
    pub total_out: f32,

    #[serde(rename = "RateIn")]
    pub rate_in: f32,

    #[serde(rename = "RateOut")]
    pub rate_out: f32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Connectedness {
    NotConnected = 0,
    Connected = 1,
    CanConnect = 2,
    CannotConnect = 3,
}

impl<'de> Deserialize<'de> for Connectedness {
    fn deserialize<D>(deserializer: D) -> Result<Connectedness, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let c: u32 = Deserialize::deserialize(deserializer)?;
        match c {
            0 => Ok(Connectedness::NotConnected),
            1 => Ok(Connectedness::Connected),
            2 => Ok(Connectedness::CanConnect),
            3 => Ok(Connectedness::CannotConnect),
            v => Err(D::Error::unknown_variant(
                &v.to_string(),
                &["0", "1", "2", "3"],
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reachability {
    Unknown = 0,
    Public = 1,
    Private = 2,
}

impl<'de> Deserialize<'de> for Reachability {
    fn deserialize<D>(deserializer: D) -> Result<Reachability, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let c: u32 = Deserialize::deserialize(deserializer)?;
        match c {
            0 => Ok(Reachability::Unknown),
            1 => Ok(Reachability::Public),
            2 => Ok(Reachability::Private),
            v => Err(D::Error::unknown_variant(&v.to_string(), &["0", "1", "2"])),
        }
    }
}

#[rpc(client)]
pub trait P2P {
    #[method(name = "p2p.BandwidthForPeer")]
    async fn p2p_bandwidth_for_peer(&self, peer_id: &PeerId) -> Result<BandwidthStats, Error>;

    #[method(name = "p2p.BandwidthForProtocol")]
    async fn p2p_bandwidth_for_protocol(&self, protocol_id: &str) -> Result<BandwidthStats, Error>;

    #[method(name = "p2p.BandwidthStats")]
    async fn p2p_bandwidth_stats(&self) -> Result<BandwidthStats, Error>;

    #[method(name = "p2p.BlockPeer")]
    async fn p2p_block_peer(&self, peer_id: &PeerId);

    #[method(name = "p2p.ClosePeer")]
    async fn p2p_close_peer(&self, peer_id: &PeerId);

    #[method(name = "p2p.Connect")]
    async fn p2p_connect(&self, address: &AddrInfo);

    #[method(name = "p2p.Connectedness")]
    async fn p2p_connectedness(&self, peer_id: &PeerId) -> Result<Connectedness, Error>;

    #[method(name = "p2p.Info")]
    async fn p2p_info(&self) -> Result<AddrInfo, Error>;

    #[method(name = "p2p.IsProtected")]
    async fn p2p_is_protected(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;

    #[method(name = "p2p.ListBlockedPeers")]
    async fn p2p_list_blocked_peers(&self) -> Result<Vec<PeerId>, Error>;

    #[method(name = "p2p.NATStatus")]
    async fn p2p_nat_status(&self) -> Result<Reachability, Error>;

    #[method(name = "p2p.PeerInfo")]
    async fn p2p_peer_info(&self, peer_id: &PeerId) -> Result<AddrInfo, Error>;

    #[method(name = "p2p.Peers")]
    async fn p2p_peers(&self) -> Result<Vec<PeerId>, Error>;

    #[method(name = "p2p.Protect")]
    async fn p2p_protect(&self, peer_id: &PeerId, tag: &str);

    #[method(name = "p2p.PubSubPeers")]
    async fn p2p_pub_sub_peers(&self, topic: &str) -> Result<Option<Vec<PeerId>>, Error>;

    #[method(name = "p2p.ResourceState")]
    async fn p2p_resource_state(&self) -> Result<ResourceManagerStats, Error>;

    #[method(name = "p2p.UnblockPeer")]
    async fn p2p_unblock_peer(&self, peer_id: &PeerId);

    #[method(name = "p2p.Unprotect")]
    async fn p2p_unprotect(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;
}
