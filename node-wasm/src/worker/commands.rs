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

pub(crate) trait CheckableResponse {
    type Output;
    fn check_variant(self) -> Result<Self::Output, WorkerError>;
}

impl<T> CheckableResponse for Result<T, WorkerResponse> {
    type Output = T;

    fn check_variant(self) -> Result<Self::Output, WorkerError> {
        self.map_err(|response| {
            error!("invalid response, received: {response:?}");
            WorkerError::InvalidResponseType
        })
    }
}

/*
#[derive(Debug, Serialize, Deserialize)]
enum WorkerErrorNg {
    NodeNotRunning,
    //NodeAlreadyRunning,
}
*/

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeStartError {}

/*
macro_rules! define_command_from_impl {
    ($common_name:ident, $command_name:ident) => {
        impl From<$command_name> for $common_name {
            fn from(command: $command_name) -> Self {
                $common_name::$command_name(command)
            }
        }
    };
}

macro_rules! define_command_type_impl {
    ($common_type:ident, $command_name:ident, $output:ty) => {
        impl $common_type for $command_name {
            type Output = $output;
        }
    };
}

macro_rules! define_response_try_from_impl {
    ($common_type:ident, $helper_type:ident, $command_name:ident) => {
        impl TryFrom<$common_type> for $helper_type<$command_name> {
            type Error = ();
            fn try_from(response: $common_type) -> Result<Self, Self::Error> {
                if let $common_type::$command_name(cmd) = response {
                    Ok(cmd)
                } else {
                    Err(())
                }
            }
        }
    };
}

macro_rules! define_command {
    ($command_name:ident -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name;
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
    ($command_name:ident ($($param:ty),+) -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name($(pub $param,)+);
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
    ($command_name:ident {$($param_name:ident : $param_type:ty),+} -> $output:ty) => {
        #[derive(Debug, Serialize, Deserialize)]
        pub struct $command_name { $(pub $param_name: $param_type,)+}
        define_command_type_impl!(NodeCommandType, $command_name, $output);
        define_command_from_impl!(NodeCommand, $command_name);
        define_response_try_from_impl!(NodeResponse, NodeCommandResponse, $command_name);
    };
}

macro_rules! define_common_types {
    ($($command_name:ident),+ $(,)?) => {
        #[allow(clippy::large_enum_variant)]
        #[derive(Serialize, Deserialize, Debug)]
        pub(crate) enum NodeCommand {
            $($command_name($command_name),)+
        }

        #[derive(Debug)]
        pub(super) enum NodeCommandWithChannel {
            $($command_name(($command_name, CommandResponseChannel<$command_name>)),)+
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub(crate) enum NodeResponse {
            $($command_name(NodeCommandResponse<$command_name>),)+
        }

        impl NodeCommand {
            pub(super) fn add_response_channel(self) -> (NodeCommandWithChannel,
        BoxFuture<'static, Result<NodeResponse, tokio::sync::oneshot::error::RecvError>>, // XXX
            ) {
                match self {
                    $(
                        NodeCommand::$command_name(cmd) => {
                            let (tx, rx) = oneshot::channel();
                            (
                                NodeCommandWithChannel::$command_name((cmd, tx)),
                                rx.map_ok(|r| NodeResponse::$command_name(
                                    NodeCommandResponse::<$command_name>(r)
                                )).boxed()
                            )
                        }
                    )+
                }
            }
        }
    };
}

define_common_types!(
    IsRunning,
    StartNode,
    GetLocalPeerId,
    GetSyncerInfo,
    GetPeerTrackerInfo,
    GetNetworkInfo,
    GetConnectedPeers,
    SetPeerTrust,
    WaitConnected,
    GetListeners,
    RequestHeader,
    GetHeader,
    GetHeadersRange,
    GetVerifiedHeaders,
    LastSeenNetworkHead,
    GetSamplingMetadata,
);

define_command!(IsRunning -> bool);
define_command!(StartNode(WasmNodeConfig) -> Result<()>);
define_command!(GetLocalPeerId -> String);
define_command!(GetSyncerInfo -> SyncingInfo);
define_command!(GetPeerTrackerInfo -> PeerTrackerInfo);
define_command!(GetNetworkInfo -> NetworkInfoSnapshot);
define_command!(GetConnectedPeers -> Vec<String>);
define_command!(SetPeerTrust { peer_id: String, is_trusted: bool } -> ());
define_command!(WaitConnected { trusted: bool } -> ());
define_command!(GetListeners -> Vec<Multiaddr>);
define_command!(RequestHeader(SingleHeaderQuery) -> Result<ExtendedHeader>);
define_command!(GetVerifiedHeaders { from: ExtendedHeader, amount: u64 } -> Vec<ExtendedHeader>);
define_command!(GetHeadersRange { start_height: Option<u64>, end_height: Option<u64> } -> Vec<ExtendedHeader>);
define_command!(GetHeader(SingleHeaderQuery) -> ExtendedHeader);
define_command!(LastSeenNetworkHead -> Option<ExtendedHeader>);
define_command!(GetSamplingMetadata { height: u64 } -> SamplingMetadata);

*/
