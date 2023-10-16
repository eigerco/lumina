//! High-level integration of [`P2p`], [`Store`], [`Syncer`].
//!
//! [`P2p`]: crate::p2p::P2p
//! [`Store`]: crate::store::Store
//! [`Syncer`]: crate::syncer::Syncer

use std::sync::Arc;

use celestia_types::hash::Hash;
use libp2p::identity::Keypair;
use libp2p::Multiaddr;

use crate::p2p::{P2p, P2pArgs, P2pError};
use crate::store::Store;
use crate::syncer::{Syncer, SyncerArgs, SyncerError};

type Result<T, E = NodeError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error(transparent)]
    P2p(#[from] P2pError),

    #[error(transparent)]
    Syncer(#[from] SyncerError),
}

pub struct NodeConfig<S>
where
    S: Store + 'static,
{
    pub network_id: String,
    pub genesis_hash: Option<Hash>,
    pub p2p_local_keypair: Keypair,
    pub p2p_bootnodes: Vec<Multiaddr>,
    pub p2p_listen_on: Vec<Multiaddr>,
    pub store: S,
}

pub struct Node<S>
where
    S: Store + 'static,
{
    p2p: Arc<P2p<S>>,
    syncer: Arc<Syncer<S>>,
}

impl<S> Node<S>
where
    S: Store,
{
    pub async fn new(config: NodeConfig<S>) -> Result<Self> {
        let store = Arc::new(config.store);

        let p2p = Arc::new(P2p::start(P2pArgs {
            network_id: config.network_id,
            local_keypair: config.p2p_local_keypair,
            bootnodes: config.p2p_bootnodes,
            listen_on: config.p2p_listen_on,
            store: store.clone(),
        })?);

        let syncer = Arc::new(Syncer::start(SyncerArgs {
            genesis_hash: config.genesis_hash,
            store: store.clone(),
            p2p: p2p.clone(),
        })?);

        Ok(Node { p2p, syncer })
    }

    pub fn p2p(&self) -> &P2p<S> {
        &self.p2p
    }

    pub fn syncer(&self) -> &Syncer<S> {
        &self.syncer
    }
}
