use std::fmt::Debug;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

use celestia_types::ExtendedHeader;
use lumina_node::node::{Node, NodeError};
use lumina_node::store::{IndexedDbStore, SamplingMetadata, Store};
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::worker::channel::{WorkerMessage, WorkerMessageServer};
use crate::worker::commands::{NodeCommand, SingleHeaderQuery, WorkerResponse};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

mod channel;
pub(crate) mod commands;

pub(crate) use channel::WorkerClient;

const WORKER_MESSAGE_SERVER_INCOMING_QUEUE_LENGTH: usize = 64;

/// actual type that's sent over a js message port
type Result<T, E = WorkerError> = std::result::Result<T, E>;

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum WorkerError {
    /*
    #[error("Command response channel has been dropped before response could be sent, should not happen")]
    SharedWorkerChannelResponseChannelDropped,
    #[error("Command channel to lumina worker closed, should not happen")]
    SharedWorkerCommandChannelClosed,
    #[error("Channel expected different response type, should not happen")]
    SharedWorkerChannelInvalidType,
    #[error("Lumina is already running, ignoring StartNode command")]
    SharedWorkerAlreadyRunning,
    #[error("Worker is still handling previous command")]
    WorkerBusy,
    */
    #[error("Node hasn't been started yet")]
    NodeNotRunning,
    #[error("Node has already been started")]
    NodeAlreadyRunning,

    #[error("node error: {0}")]
    NodeError(String),

    #[error("could not send command to the worker: {0}")]
    CommandSendingFailed(String),

    #[error("respose to command did not match expected type")]
    InvalidResponseType,

    #[error("response message could not be serialised, should not happen: {0}")]
    CouldNotSerialiseCommand(String),
    #[error("response message could not be sent: {0}")]
    CouldNotSendCommand(String),

    #[error("Received empty worker response, should not happen")]
    EmptyWorkerResponse,

    #[error("Response channel to worker closed, should not happen")]
    ResponseChannelDropped,
}

impl From<NodeError> for WorkerError {
    fn from(error: NodeError) -> Self {
        WorkerError::NodeError(error.to_string())
    }
}

struct NodeWorker {
    node: Node<IndexedDbStore>,
}

impl NodeWorker {
    async fn new(config: WasmNodeConfig) -> Self {
        let config = config.into_node_config().await.ok().unwrap();

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialised new empty store");
        }

        let node = Node::new(config).await.ok().unwrap();

        Self { node }
    }

    async fn get_syncer_info(&mut self) -> Result<SyncingInfo> {
        Ok(self.node.syncer_info().await?)
    }

    async fn get_network_info(&mut self) -> Result<NetworkInfoSnapshot> {
        Ok(self.node.network_info().await?.into())
    }

    async fn set_peer_trust(&mut self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        Ok(self.node.set_peer_trust(peer_id, is_trusted).await?)
    }

    async fn get_connected_peers(&mut self) -> Result<Vec<String>> {
        Ok(self
            .node
            .connected_peers()
            .await?
            .iter()
            .map(|id| id.to_string())
            .collect())
    }

    async fn get_listeners(&mut self) -> Result<Vec<Multiaddr>> {
        Ok(self.node.listeners().await?)
    }

    async fn wait_connected(&mut self, trusted: bool) -> Result<()> {
        if trusted {
            self.node.wait_connected().await?;
        } else {
            self.node.wait_connected_trusted().await?;
        }
        Ok(())
    }

    async fn request_header(&mut self, query: SingleHeaderQuery) -> Result<ExtendedHeader> {
        Ok(match query {
            SingleHeaderQuery::Head => self.node.request_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.request_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.request_header_by_height(height).await,
        }?)
    }

    async fn get_header(&mut self, query: SingleHeaderQuery) -> Result<ExtendedHeader> {
        Ok(match query {
            SingleHeaderQuery::Head => self.node.get_local_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.get_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.get_header_by_height(height).await,
        }?)
    }

    async fn get_verified_headers(
        &mut self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(self.node.request_verified_headers(from, amount).await?)
    }

    async fn get_headers_range(
        &mut self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(match (start_height, end_height) {
            (None, None) => self.node.get_headers(..).await,
            (Some(start), None) => self.node.get_headers(start..).await,
            (None, Some(end)) => self.node.get_headers(..=end).await,
            (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
        }?)
    }

    async fn get_sampling_metadata(&mut self, height: u64) -> Result<Option<SamplingMetadata>> {
        Ok(self.node.get_sampling_metadata(height).await?)
    }

    async fn process_command(&mut self, command: NodeCommand) -> WorkerResponse {
        match command {
            NodeCommand::IsRunning => WorkerResponse::IsRunning(true),
            NodeCommand::StartNode(_) => {
                WorkerResponse::NodeStarted(Err(WorkerError::NodeAlreadyRunning))
            }
            NodeCommand::GetLocalPeerId => {
                WorkerResponse::LocalPeerId(self.node.local_peer_id().to_string())
            }
            NodeCommand::GetSyncerInfo => WorkerResponse::SyncerInfo(self.get_syncer_info().await),
            NodeCommand::GetPeerTrackerInfo => {
                let peer_tracker_info = self.node.peer_tracker_info();
                WorkerResponse::PeerTrackerInfo(peer_tracker_info)
            }
            NodeCommand::GetNetworkInfo => {
                WorkerResponse::NetworkInfo(self.get_network_info().await)
            }
            NodeCommand::GetConnectedPeers => {
                WorkerResponse::ConnectedPeers(self.get_connected_peers().await)
            }
            NodeCommand::SetPeerTrust {
                peer_id,
                is_trusted,
            } => WorkerResponse::SetPeerTrust(self.set_peer_trust(peer_id, is_trusted).await),
            NodeCommand::WaitConnected { trusted } => {
                WorkerResponse::Connected(self.wait_connected(trusted).await)
            }
            NodeCommand::GetListeners => WorkerResponse::Listeners(self.get_listeners().await),
            NodeCommand::RequestHeader(query) => {
                WorkerResponse::Header(self.request_header(query).await)
            }
            NodeCommand::GetHeader(query) => WorkerResponse::Header(self.get_header(query).await),
            NodeCommand::GetVerifiedHeaders { from, amount } => {
                WorkerResponse::Headers(self.get_verified_headers(&from, amount).await)
            }
            NodeCommand::GetHeadersRange {
                start_height,
                end_height,
            } => WorkerResponse::Headers(self.get_headers_range(start_height, end_height).await),
            NodeCommand::LastSeenNetworkHead => {
                WorkerResponse::LastSeenNetworkHead(self.node.get_network_head_header())
            }
            NodeCommand::GetSamplingMetadata { height } => {
                WorkerResponse::SamplingMetadata(self.get_sampling_metadata(height).await)
            }
        }
    }
}

#[wasm_bindgen]
pub async fn run_worker(queued_connections: Vec<MessagePort>) {
    let (tx, mut rx) = mpsc::channel(WORKER_MESSAGE_SERVER_INCOMING_QUEUE_LENGTH);
    let mut message_server = WorkerMessageServer::new(tx.clone());

    for connection in queued_connections {
        message_server.add(connection);
    }

    let mut worker = None;
    while let Some(message) = rx.recv().await {
        match message {
            WorkerMessage::NewConnection(connection) => {
                message_server.add(connection);
            }
            WorkerMessage::Command((command, client_id)) => {
                info!("received from {client_id:?}: {command:?}");
                let Some(worker) = &mut worker else {
                    match command {
                        NodeCommand::IsRunning => {
                            message_server.respond_to(client_id, WorkerResponse::IsRunning(false));
                        }
                        NodeCommand::StartNode(config) => {
                            worker = Some(NodeWorker::new(config).await);
                            message_server
                                .respond_to(client_id, WorkerResponse::NodeStarted(Ok(())));
                        }
                        _ => {
                            warn!("Worker not running");
                            message_server.respond_err_to(client_id, WorkerError::NodeNotRunning);
                        }
                    }
                    continue;
                };

                let response = worker.process_command(command).await;
                message_server.respond_to(client_id, response);
            }
        }
    }

    error!("Channel to WorkerMessageServer closed, should not happen");
}
