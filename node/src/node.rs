//! High-level integration of [`P2p`], [`Store`], [`Syncer`].
//!
//! [`P2p`]: crate::p2p::P2p
//! [`Store`]: crate::store::Store
//! [`Syncer`]: crate::syncer::Syncer

use std::sync::Arc;

use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId};
use tokio::select;
use tokio::sync::RwLock;

use crate::p2p::{P2p, P2pConfig, P2pError};
use crate::store::Store;
use crate::syncer::Syncer;

pub struct Node {
    // TODO
}

pub struct NodeConfig {
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
}

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("P2p: {0}")]
    P2p(#[from] P2pError),
}

type Result<T, E = NodeError> = std::result::Result<T, E>;

#[allow(unused)]
struct Worker {
    store: Arc<RwLock<Store>>,
    syncer: Syncer,
    p2p: P2p,
}

#[allow(unused)]
enum NodeCmd {}

#[allow(unused)]
enum NodeEvent {}

impl Node {
    pub fn new(config: NodeConfig) -> Result<Self> {
        let store = Arc::new(RwLock::new(Store::new()));
        let syncer = Syncer::new(store.clone());

        let p2p = P2p::new(P2pConfig {
            transport: config.transport,
            store: store.clone(),
            network_id: config.network_id,
            local_keypair: config.local_keypair,
            bootstrap_peers: config.bootstrap_peers,
            listen_on: config.listen_on,
        })?;

        tokio::spawn(async move {
            Worker { store, syncer, p2p }.run().await;
        });

        Ok(Node {})
    }
}

impl Worker {
    async fn run(&mut self) {
        select! {
            Some(_ev) = self.p2p.next_event() => {
                // TODO: feed it to syncer
            }
            // TODO: receive command from `Node` and handle it
        }
    }
}
