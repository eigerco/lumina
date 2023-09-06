use libp2p::{Multiaddr, identity::ParseError};
use serde::{de, de::Error, ser, Deserialize, Serialize};
use serde_repr::Deserialize_repr;
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

#[derive(Debug, Error)]
pub enum PeerIdError {
    #[error("unable to decode base58 string")]
    Base58Error,
    #[error("libp2p error")]
    Libp2pError(#[from] ParseError),
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
