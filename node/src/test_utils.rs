use std::time::Duration;

use celestia_proto::p2p::pb::{header_request::Data, HeaderRequest};
use celestia_types::{hash::Hash, test_utils::ExtendedHeaderGenerator, ExtendedHeader};
use libp2p::identity::{self, Keypair};
use tokio::sync::{mpsc, watch};

use crate::{
    executor::timeout,
    node::NodeConfig,
    p2p::{P2pCmd, P2pError},
    peer_tracker::PeerTrackerInfo,
    store::InMemoryStore,
    utils::OneshotResultSender,
};

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

// helpers to use with struct update syntax to avoid spelling out all the details
pub fn test_node_config() -> NodeConfig<InMemoryStore> {
    let node_keypair = identity::Keypair::generate_ed25519();
    NodeConfig {
        network_id: "private".to_string(),
        genesis_hash: None,
        p2p_local_keypair: node_keypair,
        p2p_bootstrap_peers: vec![],
        p2p_listen_on: vec![],
        store: InMemoryStore::new(),
    }
}

pub fn listening_test_node_config() -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_listen_on: vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()],
        ..test_node_config()
    }
}

pub fn test_node_config_with_keypair(keypair: Keypair) -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_local_keypair: keypair,
        ..test_node_config()
    }
}

pub struct MockP2pHandle {
    pub(crate) cmd_rx: mpsc::Receiver<P2pCmd>,
    pub(crate) header_sub_tx: watch::Sender<Option<ExtendedHeader>>,
    pub(crate) peer_tracker_tx: watch::Sender<PeerTrackerInfo>,
}

impl MockP2pHandle {
    pub fn announce_peer_connected(&self) {
        self.peer_tracker_tx.send_modify(|info| {
            info.num_connected_peers += 1;
        });
    }

    pub fn announce_trusted_peer_connected(&self) {
        self.peer_tracker_tx.send_modify(|info| {
            info.num_connected_peers += 1;
            info.num_connected_trusted_peers += 1;
        });
    }

    pub fn announce_new_head(&self, header: ExtendedHeader) {
        self.header_sub_tx.send_replace(Some(header));
    }

    async fn expect_cmd(&mut self) -> P2pCmd {
        timeout(Duration::from_millis(300), async move {
            self.cmd_rx.recv().await.expect("P2p dropped")
        })
        .await
        .expect("Expecting P2pCmd, but timed-out")
    }

    pub async fn expect_no_cmd(&mut self) {
        timeout(Duration::from_millis(300), async move {
            self.cmd_rx.recv().await.expect("P2p dropped")
        })
        .await
        .expect_err("Expecting no P2pCmd, but received");
    }

    pub async fn expect_header_request_cmd(
        &mut self,
    ) -> (
        HeaderRequest,
        OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        match self.expect_cmd().await {
            P2pCmd::ExchangeHeaderRequest {
                request,
                respond_to,
            } => (request, respond_to),
            cmd => panic!("Expecting ExchangeHeaderRequest, but received: {cmd:?}"),
        }
    }

    pub async fn expect_header_request_for_height_cmd(
        &mut self,
    ) -> (u64, u64, OneshotResultSender<Vec<ExtendedHeader>, P2pError>) {
        let (req, respond_to) = self.expect_header_request_cmd().await;

        match req.data {
            Some(Data::Origin(height)) if req.amount > 0 => (height, req.amount, respond_to),
            _ => panic!("Expecting ExchangeHeaderRequest for height, but received: {req:?}"),
        }
    }

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
            _ => panic!("Expecting ExchangeHeaderRequest for hash, but received: {req:?}"),
        }
    }

    pub async fn expect_init_header_sub(
        &mut self,
    ) -> (ExtendedHeader, OneshotResultSender<(), P2pError>) {
        match self.expect_cmd().await {
            P2pCmd::InitHeaderSub { head, respond_to } => (*head, respond_to),
            cmd => panic!("Expecting InitHeaderSub, but received: {cmd:?}"),
        }
    }
}
