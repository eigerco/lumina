//! Utilities for writing tests.

use std::time::Duration;

use celestia_proto::p2p::pb::{header_request::Data, HeaderRequest};
use celestia_types::{hash::Hash, test_utils::ExtendedHeaderGenerator, ExtendedHeader};
use libp2p::identity::{self, Keypair};
use tokio::sync::{mpsc, watch};

use crate::{
    blockstore::InMemoryBlockstore,
    executor::timeout,
    network::Network,
    node::NodeBuilder,
    p2p::{P2pCmd, P2pError},
    peer_tracker::PeerTrackerInfo,
    store::InMemoryStore,
    utils::OneshotResultSender,
};

/// Generate a store pre-filled with headers.
pub fn gen_filled_store(amount: u64) -> (InMemoryStore, ExtendedHeaderGenerator) {
    let s = InMemoryStore::new();
    let mut gen = ExtendedHeaderGenerator::new();

    let headers = gen.next_many(amount);

    for header in headers {
        s.append_single_unchecked(header)
            .expect("inserting test data failed");
    }

    (s, gen)
}

/// [`NodeBuilder`] with default values for the usage in tests.
///
/// Can be used to fill the missing fields with `..test_node_config()` syntax.
pub fn test_node_builder() -> NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    let node_keypair = identity::Keypair::generate_ed25519();
    NodeBuilder::new()
        .with_network(Network::Private)
        .with_p2p_keypair(node_keypair)
        .with_blockstore(InMemoryBlockstore::new())
        .with_store(InMemoryStore::new())
}

/// [`NodeBuilder`] with listen address and default values for the usage in tests.
pub fn listening_test_node_config() -> NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    test_node_builder().with_listeners(vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()])
}

/// [`NodeBuilder`] with given keypair and default values for the usage in tests.
pub fn test_node_config_with_keypair(
    keypair: Keypair,
) -> NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    test_node_builder().with_p2p_keypair(keypair)
}

/// A handle to the mocked [`P2p`] component.
///
/// [`P2p`]: crate::p2p::P2p
pub struct MockP2pHandle {
    #[allow(dead_code)]
    pub(crate) cmd_tx: mpsc::Sender<P2pCmd>,
    pub(crate) cmd_rx: mpsc::Receiver<P2pCmd>,
    pub(crate) header_sub_tx: watch::Sender<Option<ExtendedHeader>>,
    pub(crate) peer_tracker_tx: watch::Sender<PeerTrackerInfo>,
}

impl MockP2pHandle {
    /// Simulate a new connected peer.
    pub fn announce_peer_connected(&self) {
        self.peer_tracker_tx.send_modify(|info| {
            info.num_connected_peers += 1;
        });
    }

    /// Simulate a new connected trusted peer.
    pub fn announce_trusted_peer_connected(&self) {
        self.peer_tracker_tx.send_modify(|info| {
            info.num_connected_peers += 1;
            info.num_connected_trusted_peers += 1;
        });
    }

    /// Simulate a disconnect from all peers.
    pub fn announce_all_peers_disconnected(&self) {
        self.peer_tracker_tx.send_modify(|info| {
            info.num_connected_peers = 0;
            info.num_connected_trusted_peers = 0;
        });
    }

    /// Simulate a new header announced in the network.
    pub fn announce_new_head(&self, header: ExtendedHeader) {
        self.header_sub_tx.send_replace(Some(header));
    }

    /// Assert that a command was sent to the [`P2p`] worker.
    ///
    /// [`P2p`]: crate::p2p::P2p
    async fn expect_cmd(&mut self) -> P2pCmd {
        timeout(Duration::from_millis(300), async move {
            self.cmd_rx.recv().await.expect("P2p dropped")
        })
        .await
        .expect("Expecting P2pCmd, but timed-out")
    }

    /// Assert that no command was sent to the [`P2p`] worker.
    ///
    /// [`P2p`]: crate::p2p::P2p
    pub async fn expect_no_cmd(&mut self) {
        timeout(Duration::from_millis(300), async move {
            self.cmd_rx.recv().await.expect("P2p dropped")
        })
        .await
        .expect_err("Expecting no P2pCmd, but received");
    }

    /// Assert that a header request was sent to the [`P2p`] worker and obtain a response channel.
    ///
    /// [`P2p`]: crate::p2p::P2p
    pub async fn expect_header_request_cmd(
        &mut self,
    ) -> (
        HeaderRequest,
        OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        match self.expect_cmd().await {
            P2pCmd::HeaderExRequest {
                request,
                respond_to,
            } => (request, respond_to),
            cmd => panic!("Expecting HeaderExRequest, but received: {cmd:?}"),
        }
    }

    /// Assert that a header request for height was sent to the [`P2p`] worker and obtain a response channel.
    ///
    /// [`P2p`]: crate::p2p::P2p
    pub async fn expect_header_request_for_height_cmd(
        &mut self,
    ) -> (u64, u64, OneshotResultSender<Vec<ExtendedHeader>, P2pError>) {
        let (req, respond_to) = self.expect_header_request_cmd().await;

        match req.data {
            Some(Data::Origin(height)) if req.amount > 0 => (height, req.amount, respond_to),
            _ => panic!("Expecting HeaderExRequest for height, but received: {req:?}"),
        }
    }

    /// Assert that a header request for hash was sent to the [`P2p`] worker and obtain a response channel.
    ///
    /// [`P2p`]: crate::p2p::P2p
    pub async fn expect_header_request_for_hash_cmd(
        &mut self,
    ) -> (Hash, OneshotResultSender<Vec<ExtendedHeader>, P2pError>) {
        let (req, respond_to) = self.expect_header_request_cmd().await;

        match req.data {
            Some(Data::Hash(bytes)) if req.amount == 1 => {
                let array = bytes.try_into().expect("Invalid hash");
                let hash = Hash::Sha256(array);
                (hash, respond_to)
            }
            _ => panic!("Expecting HeaderExRequest for hash, but received: {req:?}"),
        }
    }

    /// Assert that a header-sub initialization command was sent to the [`P2p`] worker.
    ///
    /// [`P2p`]: crate::p2p::P2p
    pub async fn expect_init_header_sub(&mut self) -> ExtendedHeader {
        match self.expect_cmd().await {
            P2pCmd::InitHeaderSub { head } => *head,
            cmd => panic!("Expecting InitHeaderSub, but received: {cmd:?}"),
        }
    }
}
