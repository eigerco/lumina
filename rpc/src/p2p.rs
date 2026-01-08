use jsonrpsee::proc_macros::rpc;
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

/// Information about the resources used by the node.
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

/// Statistics of the `ResourceManager` component in Go p2p nodes.
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

/// A wrapper for the libp2p `PeerId`.
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

/// Bandwidth statistics reported by the node.
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

#[rpc(client, namespace = "p2p", namespace_separator = ".")]
pub trait P2P {
    /// BandwidthForPeer returns a Stats struct with bandwidth metrics associated with the given peer.ID. The metrics returned include all traffic sent / received for the peer, regardless of protocol.
    #[method(name = "BandwidthForPeer")]
    async fn p2p_bandwidth_for_peer(&self, peer_id: &PeerId) -> Result<BandwidthStats, Error>;

    /// BandwidthForProtocol returns a Stats struct with bandwidth metrics associated with the given protocol.ID.
    #[method(name = "BandwidthForProtocol")]
    async fn p2p_bandwidth_for_protocol(&self, protocol_id: &str) -> Result<BandwidthStats, Error>;

    /// BandwidthStats returns a Stats struct with bandwidth metrics for all data sent/received by the local peer, regardless of protocol or remote peer IDs.
    #[method(name = "BandwidthStats")]
    async fn p2p_bandwidth_stats(&self) -> Result<BandwidthStats, Error>;

    /// BlockPeer adds a peer to the set of blocked peers.
    #[method(name = "BlockPeer")]
    async fn p2p_block_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    /// ClosePeer closes the connection to a given peer.
    #[method(name = "ClosePeer")]
    async fn p2p_close_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    /// Connect ensures there is a connection between this host and the peer with given peer.
    #[method(name = "Connect")]
    async fn p2p_connect(&self, address: &AddrInfo) -> Result<(), Error>;

    /// Connectedness returns a state signaling connection capabilities.
    #[method(name = "Connectedness")]
    async fn p2p_connectedness(&self, peer_id: &PeerId) -> Result<Connectedness, Error>;

    /// Info returns address information about the host.
    #[method(name = "Info")]
    async fn p2p_info(&self) -> Result<AddrInfo, Error>;

    /// IsProtected returns whether the given peer is protected.
    #[method(name = "IsProtected")]
    async fn p2p_is_protected(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;

    /// ListBlockedPeers returns a list of blocked peers.
    #[method(name = "ListBlockedPeers")]
    async fn p2p_list_blocked_peers(&self) -> Result<Vec<PeerId>, Error>;

    /// NATStatus returns the current NAT status.
    #[method(name = "NATStatus")]
    async fn p2p_nat_status(&self) -> Result<Reachability, Error>;

    /// PeerInfo returns a small slice of information Peerstore has on the given peer.
    #[method(name = "PeerInfo")]
    async fn p2p_peer_info(&self, peer_id: &PeerId) -> Result<AddrInfo, Error>;

    /// Peers returns connected peers.
    #[method(name = "Peers")]
    async fn p2p_peers(&self) -> Result<Vec<PeerId>, Error>;

    /// Protect adds a peer to the list of peers who have a bidirectional peering agreement that they are protected from being trimmed, dropped or negatively scored.
    #[method(name = "Protect")]
    async fn p2p_protect(&self, peer_id: &PeerId, tag: &str) -> Result<(), Error>;

    // We might get null in response here, so Option is needed
    /// PubSubPeers returns the peer IDs of the peers joined on the given topic.
    #[method(name = "PubSubPeers")]
    async fn p2p_pub_sub_peers(&self, topic: &str) -> Result<Option<Vec<PeerId>>, Error>;

    /// ResourceState returns the state of the resource manager.
    #[method(name = "ResourceState")]
    async fn p2p_resource_state(&self) -> Result<ResourceManagerStats, Error>;

    /// UnblockPeer removes a peer from the set of blocked peers.
    #[method(name = "UnblockPeer")]
    async fn p2p_unblock_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    /// Unprotect removes a peer from the list of peers who have a bidirectional peering agreement that they are protected from being trimmed, dropped or negatively scored, returning a bool representing whether the given peer is protected or not.
    #[method(name = "Unprotect")]
    async fn p2p_unprotect(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;
}
