//! High-level integration of [`P2p`], [`Store`], [`Syncer`].
//!
//! [`P2p`]: crate::p2p::P2p
//! [`Store`]: crate::store::Store
//! [`Syncer`]: crate::syncer::Syncer

use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;

use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId};

use crate::p2p::{P2p, P2pArgs, P2pService};
use crate::store::Store;
use crate::syncer::{Syncer, SyncerArgs, SyncerService};

#[derive(Debug, thiserror::Error)]
pub enum NodeError<P2pSrv, SyncerSrv, S>
where
    S: Store,
    P2pSrv: P2pService<S>,
    SyncerSrv: SyncerService<P2pSrv, S>,
{
    #[error(transparent)]
    P2pService(P2pSrv::Error),

    #[error(transparent)]
    SyncerService(SyncerSrv::Error),

    #[error("should not happen")]
    _Unreachable(Infallible, PhantomData<S>),
}

pub struct NodeConfig<S: Store> {
    pub network_id: String,
    pub p2p_transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub p2p_local_keypair: Keypair,
    pub p2p_bootstrap_peers: Vec<Multiaddr>,
    pub p2p_listen_on: Vec<Multiaddr>,
    pub store: S,
}

pub type Node<S> = GenericNode<P2p<S>, Syncer<P2p<S>, S>, S>;

pub struct GenericNode<P2pSrv, SyncerSrv, S>
where
    S: Store,
    P2pSrv: P2pService<S>,
    SyncerSrv: SyncerService<P2pSrv, S>,
{
    p2p: Arc<P2pSrv>,
    syncer: Arc<SyncerSrv>,
    _store: PhantomData<S>,
}

impl<P2pSrv, SyncerSrv, S> GenericNode<P2pSrv, SyncerSrv, S>
where
    S: Store,
    P2pSrv: P2pService<S>,
    SyncerSrv: SyncerService<P2pSrv, S>,
{
    pub async fn new(config: NodeConfig<S>) -> Result<Self, NodeError<P2pSrv, SyncerSrv, S>> {
        let store = Arc::new(config.store);

        let p2p = Arc::new(
            P2pSrv::start(P2pArgs {
                network_id: config.network_id,
                transport: config.p2p_transport,
                local_keypair: config.p2p_local_keypair,
                bootstrap_peers: config.p2p_bootstrap_peers,
                listen_on: config.p2p_listen_on,
                store: store.clone(),
            })
            .await
            .map_err(NodeError::P2pService)?,
        );

        let syncer = Arc::new(
            SyncerSrv::start(SyncerArgs {
                store: store.clone(),
                p2p: p2p.clone(),
            })
            .await
            .map_err(NodeError::SyncerService)?,
        );

        Ok(GenericNode {
            p2p,
            syncer,
            _store: PhantomData,
        })
    }

    pub fn p2p(&self) -> &impl P2pService<S> {
        &*self.p2p
    }

    pub fn syncer(&self) -> &impl SyncerService<P2pSrv, S> {
        &*self.syncer
    }
}
