use std::fmt::Debug;

use enum_as_inner::EnumAsInner;
//use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use libp2p::Multiaddr;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tracing::error;

use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use lumina_node::peer_tracker::PeerTrackerInfo;
use lumina_node::store::SamplingMetadata;
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::worker::Result;
use crate::worker::WorkerError;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeCommand {
    IsRunning,
    StartNode(WasmNodeConfig),
    GetLocalPeerId,
    GetSyncerInfo,
    GetPeerTrackerInfo,
    GetNetworkInfo,
    GetConnectedPeers,
    SetPeerTrust {
        peer_id: PeerId,
        is_trusted: bool,
    },
    WaitConnected {
        trusted: bool,
    },
    GetListeners,
    RequestHeader(SingleHeaderQuery),
    GetVerifiedHeaders {
        from: ExtendedHeader,
        amount: u64,
    },
    GetHeadersRange {
        start_height: Option<u64>,
        end_height: Option<u64>,
    },
    GetHeader(SingleHeaderQuery),
    LastSeenNetworkHead,
    GetSamplingMetadata {
        height: u64,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum SingleHeaderQuery {
    Head,
    ByHash(Hash),
    ByHeight(u64),
}

#[derive(Serialize, Deserialize, Debug, EnumAsInner)]
pub(crate) enum WorkerResponse {
    IsRunning(bool),
    NodeStarted(Result<()>),
    LocalPeerId(String),
    SyncerInfo(Result<SyncingInfo>),
    PeerTrackerInfo(PeerTrackerInfo),
    NetworkInfo(Result<NetworkInfoSnapshot>),
    ConnectedPeers(Result<Vec<String>>),
    SetPeerTrust(Result<()>),
    Connected(Result<()>),
    Listeners(Result<Vec<Multiaddr>>),
    Header(Result<ExtendedHeader>),
    Headers(Result<Vec<ExtendedHeader>>),
    LastSeenNetworkHead(Option<ExtendedHeader>),
    SamplingMetadata(Result<Option<SamplingMetadata>>),
}

pub(crate) trait CheckableResponseExt {
    type Output;
    fn check_variant(self) -> Result<Self::Output, WorkerError>;
}

impl<T> CheckableResponseExt for Result<T, WorkerResponse> {
    type Output = T;

    fn check_variant(self) -> Result<Self::Output, WorkerError> {
        self.map_err(|response| {
            error!("invalid response, received: {response:?}");
            WorkerError::InvalidResponseType
        })
    }
}
