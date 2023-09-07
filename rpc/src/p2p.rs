use celestia_types::p2p::{
    AddrInfo, BandwidthStats, Connectedness, PeerId, Reachability, ResourceManagerStats,
};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait P2P {
    /// BandwidthForPeer returns a Stats struct with bandwidth metrics associated with the given peer.ID. The metrics returned include all traffic sent / received for the peer, regardless of protocol.
    #[method(name = "p2p.BandwidthForPeer")]
    async fn p2p_bandwidth_for_peer(&self, peer_id: &PeerId) -> Result<BandwidthStats, Error>;

    /// BandwidthForProtocol returns a Stats struct with bandwidth metrics associated with the given protocol.ID.
    #[method(name = "p2p.BandwidthForProtocol")]
    async fn p2p_bandwidth_for_protocol(&self, protocol_id: &str) -> Result<BandwidthStats, Error>;

    /// BandwidthStats returns a Stats struct with bandwidth metrics for all data sent/received by the local peer, regardless of protocol or remote peer IDs.
    #[method(name = "p2p.BandwidthStats")]
    async fn p2p_bandwidth_stats(&self) -> Result<BandwidthStats, Error>;

    // This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    /// BlockPeer adds a peer to the set of blocked peers.
    #[method(name = "p2p.BlockPeer")]
    async fn p2p_block_peer(&self, peer_id: &PeerId);

    // This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    /// ClosePeer closes the connection to a given peer.
    #[method(name = "p2p.ClosePeer")]
    async fn p2p_close_peer(&self, peer_id: &PeerId);

    // This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    /// Connect ensures there is a connection between this host and the peer with given peer.
    #[method(name = "p2p.Connect")]
    async fn p2p_connect(&self, address: &AddrInfo);

    /// Connectedness returns a state signaling connection capabilities.
    #[method(name = "p2p.Connectedness")]
    async fn p2p_connectedness(&self, peer_id: &PeerId) -> Result<Connectedness, Error>;

    /// Info returns address information about the host.
    #[method(name = "p2p.Info")]
    async fn p2p_info(&self) -> Result<AddrInfo, Error>;

    /// IsProtected returns whether the given peer is protected.
    #[method(name = "p2p.IsProtected")]
    async fn p2p_is_protected(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;

    /// ListBlockedPeers returns a list of blocked peers.
    #[method(name = "p2p.ListBlockedPeers")]
    async fn p2p_list_blocked_peers(&self) -> Result<Vec<PeerId>, Error>;

    /// NATStatus returns the current NAT status.
    #[method(name = "p2p.NATStatus")]
    async fn p2p_nat_status(&self) -> Result<Reachability, Error>;

    /// PeerInfo returns a small slice of information Peerstore has on the given peer.
    #[method(name = "p2p.PeerInfo")]
    async fn p2p_peer_info(&self, peer_id: &PeerId) -> Result<AddrInfo, Error>;

    /// Peers returns connected peers.
    #[method(name = "p2p.Peers")]
    async fn p2p_peers(&self) -> Result<Vec<PeerId>, Error>;

    // This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    /// Protect adds a peer to the list of peers who have a bidirectional peering agreement that they are protected from being trimmed, dropped or negatively scored.
    #[method(name = "p2p.Protect")]
    async fn p2p_protect(&self, peer_id: &PeerId, tag: &str);

    // We might get null in response here, so Option is needed
    /// PubSubPeers returns the peer IDs of the peers joined on the given topic.
    #[method(name = "p2p.PubSubPeers")]
    async fn p2p_pub_sub_peers(&self, topic: &str) -> Result<Option<Vec<PeerId>>, Error>;

    /// ResourceState returns the state of the resource manager.
    #[method(name = "p2p.ResourceState")]
    async fn p2p_resource_state(&self) -> Result<ResourceManagerStats, Error>;

    // This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    /// UnblockPeer removes a peer from the set of blocked peers.
    #[method(name = "p2p.UnblockPeer")]
    async fn p2p_unblock_peer(&self, peer_id: &PeerId);

    /// Unprotect removes a peer from the list of peers who have a bidirectional peering agreement that they are protected from being trimmed, dropped or negatively scored, returning a bool representing whether the given peer is protected or not.
    #[method(name = "p2p.Unprotect")]
    async fn p2p_unprotect(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;
}
