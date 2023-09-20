use std::sync::Arc;

use async_trait::async_trait;

use crate::p2p::P2pService;
use crate::Service;

type Result<T, E = SyncerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum SyncerError {}

#[allow(unused)]
#[derive(Debug)]
pub struct Syncer<P2pSrv>
where
    P2pSrv: P2pService,
{
    p2p: Arc<P2pSrv>,
    store: Arc<P2pSrv::Store>,
}

pub struct SyncerArgs<P2pSrv>
where
    P2pSrv: P2pService,
{
    pub p2p: Arc<P2pSrv>,
    pub store: Arc<P2pSrv::Store>,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum SyncerCmd {}

#[async_trait]
impl<P2pSrv> Service for Syncer<P2pSrv>
where
    P2pSrv: P2pService,
{
    type Command = SyncerCmd;
    type Args = SyncerArgs<P2pSrv>;
    type Error = SyncerError;

    async fn start(args: SyncerArgs<P2pSrv>) -> Result<Self, SyncerError> {
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
pub trait SyncerService<P2pSrv>:
    Service<Args = SyncerArgs<P2pSrv>, Command = SyncerCmd, Error = SyncerError>
where
    P2pSrv: P2pService,
{
}

#[async_trait]
impl<P2pSrv> SyncerService<P2pSrv> for Syncer<P2pSrv> where P2pSrv: P2pService {}
