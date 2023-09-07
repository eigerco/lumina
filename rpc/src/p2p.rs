use celestia_types::p2p::{
    AddrInfo, BandwidthStats, Connectedness, PeerId, Reachability, ResourceManagerStats,
};
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait P2P {
    #[method(name = "p2p.BandwidthForPeer")]
    async fn p2p_bandwidth_for_peer(&self, peer_id: &PeerId) -> Result<BandwidthStats, Error>;

    #[method(name = "p2p.BandwidthForProtocol")]
    async fn p2p_bandwidth_for_protocol(&self, protocol_id: &str) -> Result<BandwidthStats, Error>;

    #[method(name = "p2p.BandwidthStats")]
    async fn p2p_bandwidth_stats(&self) -> Result<BandwidthStats, Error>;

    /// This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    #[method(name = "p2p.BlockPeer")]
    async fn p2p_block_peer(&self, peer_id: &PeerId);

    /// This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    #[method(name = "p2p.ClosePeer")]
    async fn p2p_close_peer(&self, peer_id: &PeerId);

    /// This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
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

    /// This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    #[method(name = "p2p.Protect")]
    async fn p2p_protect(&self, peer_id: &PeerId, tag: &str);

    #[method(name = "p2p.PubSubPeers")]
    async fn p2p_pub_sub_peers(&self, topic: &str) -> Result<Option<Vec<PeerId>>, Error>;

    #[method(name = "p2p.ResourceState")]
    async fn p2p_resource_state(&self) -> Result<ResourceManagerStats, Error>;

    /// This method does not report errors due to a workaround to a go-jsonrpc bug, see https://github.com/eigerco/celestia-node-rs/issues/53
    #[method(name = "p2p.UnblockPeer")]
    async fn p2p_unblock_peer(&self, peer_id: &PeerId);

    #[method(name = "p2p.Unprotect")]
    async fn p2p_unprotect(&self, peer_id: &PeerId, tag: &str) -> Result<bool, Error>;
}
