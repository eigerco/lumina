//! Types related to the p2p layer of nodes in Celestia.

use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;

/// An id and addresses of a peer in p2p network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AddrInfo {
    /// An id of a peer.
    #[serde(rename = "ID")]
    pub id: PeerId,
    /// A list of addresses where peer listens on.
    pub addrs: Vec<Multiaddr>,
}

/// Information about the resources used by a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Stat {
    /// Amount of inbound streams.
    pub num_streams_inbound: u32,
    /// Amount of outbound streams.
    pub num_streams_outbound: u32,
    /// Amount of inbound connections.
    pub num_conns_inbound: u32,
    /// Amount of outbound connections.
    pub num_conns_outbound: u32,
    /// Amound of open file descriptors.
    #[serde(rename = "NumFD")]
    pub num_fd: u32,
    /// A memory usage.
    pub memory: u32,
}

/// Statistics of the [`ResourceManager`] component in Go p2p nodes.
///
/// [`ResourceManager`]: https://github.com/libp2p/go-libp2p/tree/master/p2p/host/resource-manager
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourceManagerStats {
    /// System statistics.
    pub system: Stat,
    /// Transient statistics.
    pub transient: Stat,
    /// Statistics per service.
    pub services: HashMap<String, Stat>,
    /// Statistics per protocol.
    pub protocols: HashMap<String, Stat>,
    /// Statistics per peer.
    pub peers: HashMap<String, Stat>,
}

/// A wrapper for the libp2p [`PeerId`].
///
/// [`PeerId`]: libp2p_identity::PeerId
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

/// Bandwidth statistics reported by a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BandwidthStats {
    /// Total bytes received.
    pub total_in: f32,
    /// Total bytes sent.
    pub total_out: f32,
    /// Rate of receiving.
    pub rate_in: f32,
    /// Rate of sending.
    pub rate_out: f32,
}

/// A representation of the connection status and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Connectedness {
    /// Not connected.
    NotConnected = 0,
    /// Connected.
    Connected = 1,
    /// Not connected but connection is possible.
    CanConnect = 2,
    /// Not connected and connection is impossible.
    CannotConnect = 3,
}

/// A representation of the peer's reachability in network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Reachability {
    /// Unknown address.
    Unknown = 0,
    /// Public address.
    Public = 1,
    /// Private address.
    Private = 2,
}
