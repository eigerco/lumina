use std::fmt::Debug;

use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use lumina_node::peer_tracker::PeerTrackerInfo;
use lumina_node::store::SamplingMetadata;
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::worker::Result;
use crate::worker::{CommandResponseChannel, NodeCommandResponse, NodeCommandType};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

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
    RequestMultipleHeaders,
    GetHeader,
    GetMultipleHeaders,
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
define_command!(RequestMultipleHeaders(MultipleHeaderQuery) -> Vec<ExtendedHeader>);
define_command!(GetHeader(SingleHeaderQuery) -> ExtendedHeader);
define_command!(GetMultipleHeaders(MultipleHeaderQuery) -> Vec<ExtendedHeader>);
define_command!(LastSeenNetworkHead -> Option<ExtendedHeader>);
define_command!(GetSamplingMetadata { height: u64 } -> SamplingMetadata);

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum SingleHeaderQuery {
    Head,
    ByHash(Hash),
    ByHeight(u64),
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum MultipleHeaderQuery {
    GetVerified {
        from: ExtendedHeader,
        amount: u64,
    },
    Range {
        start_height: Option<u64>,
        end_height: Option<u64>,
    },
}
