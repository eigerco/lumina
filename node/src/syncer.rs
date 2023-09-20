use std::sync::Arc;

use async_trait::async_trait;

use crate::p2p::P2pService;
use crate::store::Store;
use crate::Service;

type Result<T, E = SyncerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum SyncerError {}

#[allow(unused)]
#[derive(Debug)]
pub struct Syncer<P2pSrv, S>
where
    S: Store,
    P2pSrv: P2pService<S>,
{
    p2p: Arc<P2pSrv>,
    store: Arc<S>,
}

pub struct SyncerArgs<P2pSrv, S>
where
    S: Store,
    P2pSrv: P2pService<S>,
{
    pub p2p: Arc<P2pSrv>,
    pub store: Arc<S>,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum SyncerCmd {}

#[async_trait]
impl<P2pSrv, S> Service for Syncer<P2pSrv, S>
where
    S: Store + Sync + Send,
    P2pSrv: P2pService<S>,
{
    type Command = SyncerCmd;
    type Args = SyncerArgs<P2pSrv, S>;
    type Error = SyncerError;

    async fn start(args: SyncerArgs<P2pSrv, S>) -> Result<Self, SyncerError> {
        // TODO
        Ok(Self {
            p2p: args.p2p,
            store: args.store,
        })
    }

    async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    async fn send_command(&self, _cmd: SyncerCmd) -> Result<()> {
        // TODO
        Ok(())
    }
}

#[async_trait]
pub trait SyncerService<P2pSrv, S>:
    Service<Args = SyncerArgs<P2pSrv, S>, Command = SyncerCmd, Error = SyncerError>
where
    S: Store,
    P2pSrv: P2pService<S>,
{
}

#[async_trait]
impl<P2pSrv, S> SyncerService<P2pSrv, S> for Syncer<P2pSrv, S>
where
    S: Store + Sync + Send,
    P2pSrv: P2pService<S>,
{
}
