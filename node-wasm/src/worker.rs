use std::fmt::Debug;

use js_sys::Array;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, MessagePort, SharedWorker, Url, WorkerOptions, WorkerType};

use celestia_tendermint::error::Error as TendermintError;
use libp2p::multiaddr::Error as MultiaddrError;
use lumina_node::node::{Node, NodeError};
use lumina_node::store::{IndexedDbStore, SamplingMetadata, Store, StoreError};
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
    #[error("node hasn't been started yet")]
    NodeNotRunning,
    #[error("node has already been started")]
    NodeAlreadyRunning,
    #[error("could not create blockstore: {0}")]
    BlockstoreCrationFailed(String),
    #[error("could not create header store: {0}")]
    StoreCrationFailed(String),
    #[error("could not parse genesis hash: {0}")]
    GenesisHashInvalid(String),
    #[error("could not parse bootstrap node multiaddr: {0}")]
    BootstrapNodeMultiaddrInvalid(String),
    #[error("node error: {0}")]
    NodeError(String),
    #[error("respose to command did not match expected type, should not happen")]
    InvalidResponseType,
    #[error("Value could not be serialized, should not happen: {0}")]
    SerdeError(String),
    #[error("command message could not be serialised, should not happen: {0}")]
    CouldNotSerialiseCommand(String),
    #[error("command response could not be serialised, should not happen: {0}")]
    CouldNotSerialiseResponseValue(String),
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

impl From<blockstore::Error> for WorkerError {
    fn from(error: blockstore::Error) -> Self {
        WorkerError::BlockstoreCrationFailed(error.to_string())
    }
}

impl From<StoreError> for WorkerError {
    fn from(error: StoreError) -> Self {
        WorkerError::StoreCrationFailed(error.to_string())
    }
}

impl From<TendermintError> for WorkerError {
    fn from(error: TendermintError) -> Self {
        WorkerError::GenesisHashInvalid(error.to_string())
    }
}

impl From<MultiaddrError> for WorkerError {
    fn from(error: MultiaddrError) -> Self {
        WorkerError::BootstrapNodeMultiaddrInvalid(error.to_string())
    }
}

impl From<serde_wasm_bindgen::Error> for WorkerError {
    fn from(error: serde_wasm_bindgen::Error) -> Self {
        WorkerError::SerdeError(error.to_string())
    }
}
struct NodeWorker {
    node: Node<IndexedDbStore>,
}

fn to_jsvalue_or_undefined<T: Serialize>(value: &T) -> JsValue {
    to_value(value).unwrap()
}

impl NodeWorker {
    async fn new(config: WasmNodeConfig) -> Result<Self, WorkerError> {
        let config = config.into_node_config().await?;

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialised new empty store");
        }

        let node = Node::new(config).await?;

        Ok(Self { node })
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

    async fn request_header(&mut self, query: SingleHeaderQuery) -> Result<JsValue> {
        let header = match query {
            SingleHeaderQuery::Head => self.node.request_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.request_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.request_header_by_height(height).await,
        }?;
        Ok(to_value(&header)
            .map_err(|e| WorkerError::CouldNotSerialiseResponseValue(e.to_string()))?)
    }

    async fn get_header(&mut self, query: SingleHeaderQuery) -> Result<JsValue> {
        let header = match query {
            SingleHeaderQuery::Head => self.node.get_local_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.get_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.get_header_by_height(height).await,
        }?;
        Ok(to_value(&header)
            .map_err(|e| WorkerError::CouldNotSerialiseResponseValue(e.to_string()))?)
    }

    async fn get_verified_headers(&mut self, from: JsValue, amount: u64) -> Result<Array> {
        let verified_headers = self
            .node
            .request_verified_headers(&from_value(from)?, amount)
            .await?;
        Ok(verified_headers
            .iter()
            .map(|h| to_value(&h))
            .collect::<Result<Array, _>>()?)
    }

    async fn get_headers_range(
        &mut self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Array> {
        let headers = match (start_height, end_height) {
            (None, None) => self.node.get_headers(..).await,
            (Some(start), None) => self.node.get_headers(start..).await,
            (None, Some(end)) => self.node.get_headers(..=end).await,
            (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
        }?;

        Ok(headers
            .iter()
            .map(|h| to_value(&h))
            .collect::<Result<Array, _>>()?)
    }

    async fn get_last_seen_network_head(&mut self) -> JsValue {
        self.node
            .get_network_head_header()
            .as_ref()
            .map(to_jsvalue_or_undefined)
            .unwrap_or(JsValue::UNDEFINED)
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
                WorkerResponse::Header(self.request_header(query).await.into())
            }
            NodeCommand::GetHeader(query) => {
                WorkerResponse::Header(self.get_header(query).await.into())
            }
            NodeCommand::GetVerifiedHeaders { from, amount } => {
                WorkerResponse::Headers(self.get_verified_headers(from, amount).await.into())
            }
            NodeCommand::GetHeadersRange {
                start_height,
                end_height,
            } => WorkerResponse::Headers(
                self.get_headers_range(start_height, end_height)
                    .await
                    .into(),
            ),
            NodeCommand::LastSeenNetworkHead => {
                WorkerResponse::LastSeenNetworkHead(self.get_last_seen_network_head().await)
            }
            NodeCommand::GetSamplingMetadata { height } => {
                WorkerResponse::SamplingMetadata(self.get_sampling_metadata(height).await)
            }
        }
    }
}

#[wasm_bindgen]
pub async fn run_worker(queued_connections: Vec<MessagePort>) {
    info!("Entered run_worker");
    let (tx, mut rx) = mpsc::channel(WORKER_MESSAGE_SERVER_INCOMING_QUEUE_LENGTH);
    let mut message_server = WorkerMessageServer::new(tx.clone());

    for connection in queued_connections {
        message_server.add(connection);
    }

    info!("Entering SharedWorker message loop");
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
                            match NodeWorker::new(config).await {
                                Ok(node) => {
                                    worker = Some(node);
                                    message_server
                                        .respond_to(client_id, WorkerResponse::NodeStarted(Ok(())));
                                }
                                Err(e) => {
                                    message_server
                                        .respond_to(client_id, WorkerResponse::NodeStarted(Err(e)));
                                }
                            };
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

/// SharedWorker can only be spawned from an [`URL`]. To eliminate a need to host a js shim
/// which calls into our Rust wasm code under specific path, we encode its entire contents
/// as a [`Blob`] which is then passed as an URL.
///
/// [`URL`]: https://developer.mozilla.org/en-US/docs/Web/API/URL
/// ['Blob']: https://developer.mozilla.org/en-US/docs/Web/API/Blob

pub(crate) fn spawn_worker(name: &str, wasm_url: &str) -> Result<SharedWorker, JsError> {
    let script = format!(
        r#"
Error.stackTraceLimit = 99;

let queued = [];
onconnect = (event) => {{
  console.log("Queued connection", event);
  queued.push(event.ports[0]);
}}

self.lumina = await import(self.location.origin + '{wasm_url}');
await self.lumina.default();
await self.lumina.run_worker(queued);
"#
    );

    info!("js shim: {script}");

    let array = Array::new();
    array.push(&script.into());
    let blob = Blob::new_with_str_sequence_and_options(
        &array,
        BlobPropertyBag::new().type_("application/javascript"),
    )
    .map_err(|e| JsError::new(&format!("could not create SharedWorker Blob: {e:?}")))?;

    let url = Url::create_object_url_with_blob(&blob)
        .map_err(|e| JsError::new(&format!("could not create SharedWorker Url: {e:?}")))?;

    let mut opts = WorkerOptions::new();
    opts.type_(WorkerType::Module);
    opts.name(name);

    let worker = SharedWorker::new_with_worker_options(&url, &opts)
        .map_err(|e| JsError::new(&format!("could not create SharedWorker Url: {e:?}")))?;

    Ok(worker)
}
