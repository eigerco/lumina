use std::fmt::Debug;

use enum_as_inner::EnumAsInner;
use js_sys::Array;
use libp2p::Multiaddr;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tracing::error;
use wasm_bindgen::{JsError, JsValue};

use celestia_types::hash::Hash;
use lumina_node::peer_tracker::PeerTrackerInfo;
use lumina_node::store::SamplingMetadata;
use lumina_node::syncer::SyncingInfo;

use crate::error::Error;
use crate::error::Result;
use crate::node::WasmNodeConfig;
use crate::utils::JsResult;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeCommand {
    IsRunning,
    StartNode(WasmNodeConfig),
    GetEventsChannelName,
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
        #[serde(with = "serde_wasm_bindgen::preserve")]
        from: JsValue,
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
    CloseWorker,
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
    EventsChannelName(String),
    LocalPeerId(String),
    SyncerInfo(Result<SyncingInfo>),
    PeerTrackerInfo(PeerTrackerInfo),
    NetworkInfo(Result<NetworkInfoSnapshot>),
    ConnectedPeers(Result<Vec<String>>),
    SetPeerTrust(Result<()>),
    Connected(Result<()>),
    Listeners(Result<Vec<Multiaddr>>),
    Header(JsResult<JsValue, Error>),
    Headers(JsResult<Array, Error>),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    LastSeenNetworkHead(JsValue),
    SamplingMetadata(Result<Option<SamplingMetadata>>),
    WorkerClosed(()),
}

pub(crate) trait CheckableResponseExt {
    type Output;

    fn check_variant(self) -> Result<Self::Output, JsError>;
}

impl<T> CheckableResponseExt for Result<T, WorkerResponse> {
    type Output = T;

    fn check_variant(self) -> Result<Self::Output, JsError> {
        self.map_err(|response| {
            error!("invalid response, received: {response:?}");
            JsError::new("invalid response received for the command sent")
        })
    }
}
