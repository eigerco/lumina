use std::fmt::Debug;
use std::time::Duration;

use blockstore::EitherBlockstore;
use celestia_types::nmt::Namespace;
use celestia_types::Blob;
use js_sys::Array;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::BroadcastChannel;

use lumina_node::blockstore::{InMemoryBlockstore, IndexedDbBlockstore};
use lumina_node::events::{EventSubscriber, NodeEventInfo};
use lumina_node::node::{Node, SyncingInfo};
use lumina_node::store::{EitherStore, InMemoryStore, IndexedDbStore, SamplingMetadata};

use crate::client::WasmNodeConfig;
use crate::commands::{NodeCommand, SingleHeaderQuery, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::ports::{ClientMessage, WorkerServer};
use crate::utils::random_id;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

pub(crate) type WasmBlockstore = EitherBlockstore<InMemoryBlockstore, IndexedDbBlockstore>;
pub(crate) type WasmStore = EitherStore<InMemoryStore, IndexedDbStore>;

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum WorkerError {
    /// Worker is initialised, but the node has not been started yet. Use [`NodeDriver::start`].
    #[error("node hasn't been started yet")]
    NodeNotRunning,
    /// Communication with worker has been broken and we're unable to send or receive messages from it.
    /// Try creating new [`NodeDriver`] instance.
    #[error("error trying to communicate with worker")]
    WorkerCommunicationError(Error),
    /// Worker received unrecognised command
    #[error("invalid command received")]
    InvalidCommandReceived,
    /// Worker encountered error coming from lumina-node
    #[error("Worker encountered an error: {0:?}")]
    NodeError(Error),
}

/// `NodeWorker` is responsible for receiving commands from connected [`NodeClient`]s, executing
/// them and sending a response back, as well as accepting new `NodeClient` connections.
///
/// [`NodeClient`]: crate::client::NodeClient
#[wasm_bindgen]
pub struct NodeWorker {
    event_channel_name: String,
    node: Option<NodeWorkerInstance>,
    request_server: WorkerServer,
    _control_channel: mpsc::UnboundedSender<ClientMessage>,
}

struct NodeWorkerInstance {
    node: Node<WasmBlockstore, WasmStore>,
    events_channel_name: String,
}

#[wasm_bindgen]
impl NodeWorker {
    #[wasm_bindgen(constructor)]
    pub fn new(port_like_object: JsValue) -> Self {
        info!("Created lumina worker");

        let request_server = WorkerServer::new();
        let control_channel = request_server.get_control_channel();

        control_channel
            .send(ClientMessage::AddConnection(port_like_object))
            .expect("control channel should be ready to receive now");

        Self {
            event_channel_name: format!("NodeEventChannel-{}", random_id()),
            node: None,
            request_server,
            _control_channel: control_channel,
        }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        loop {
            let (client_id, command) = self.request_server.recv().await?;

            // StopNode needs special handling because `NodeWorkerInstance` needs to be consumed.
            if matches!(&command, NodeCommand::StopNode) {
                if let Some(node) = self.node.take() {
                    node.stop().await;
                    self.request_server
                        .respond_to(client_id, WorkerResponse::NodeStopped(()));
                    continue;
                }
            }

            let response = match &mut self.node {
                Some(node) => node.process_command(command).await,
                node @ None => match command {
                    NodeCommand::InternalPing => WorkerResponse::InternalPong,
                    NodeCommand::IsRunning => WorkerResponse::IsRunning(false),
                    NodeCommand::GetEventsChannelName => {
                        WorkerResponse::EventsChannelName(self.event_channel_name.clone())
                    }
                    NodeCommand::StartNode(config) => {
                        match NodeWorkerInstance::new(&self.event_channel_name, config).await {
                            Ok(instance) => {
                                let _ = node.insert(instance);
                                WorkerResponse::NodeStarted(Ok(()))
                            }
                            Err(e) => WorkerResponse::NodeStarted(Err(e)),
                        }
                    }
                    _ => {
                        warn!("Worker not running");
                        WorkerResponse::NodeNotRunning
                    }
                },
            };

            self.request_server.respond_to(client_id, response);
        }
    }
}

impl NodeWorkerInstance {
    async fn new(events_channel_name: &str, config: WasmNodeConfig) -> Result<Self> {
        let (node, events_sub) = config.into_node_builder().await?.start_subscribed().await?;

        let events_channel = BroadcastChannel::new(events_channel_name)
            .context("Failed to allocate BroadcastChannel")?;

        spawn_local(event_forwarder_task(events_sub, events_channel));

        Ok(Self {
            node,
            events_channel_name: events_channel_name.to_owned(),
        })
    }

    async fn stop(self) {
        self.node.stop().await;
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
            self.node.wait_connected_trusted().await?;
        } else {
            self.node.wait_connected().await?;
        }
        Ok(())
    }

    async fn request_header(&mut self, query: SingleHeaderQuery) -> Result<JsValue> {
        let header = match query {
            SingleHeaderQuery::Head => self.node.request_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.request_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.request_header_by_height(height).await,
        }?;
        to_value(&header).context("could not serialise requested header")
    }

    async fn get_header(&mut self, query: SingleHeaderQuery) -> Result<JsValue> {
        let header = match query {
            SingleHeaderQuery::Head => self.node.get_local_head_header().await,
            SingleHeaderQuery::ByHash(hash) => self.node.get_header_by_hash(&hash).await,
            SingleHeaderQuery::ByHeight(height) => self.node.get_header_by_height(height).await,
        }?;
        to_value(&header).context("could not serialise requested header")
    }

    async fn get_verified_headers(&mut self, from: JsValue, amount: u64) -> Result<Array> {
        let verified_headers = self
            .node
            .request_verified_headers(
                &from_value(from).context("could not deserialise verified header")?,
                amount,
            )
            .await?;
        verified_headers
            .iter()
            .map(|h| to_value(&h))
            .collect::<Result<Array, _>>()
            .context("could not serialise fetched headers")
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

        headers
            .iter()
            .map(|h| to_value(&h))
            .collect::<Result<Array, _>>()
            .context("could not serialise fetched headers")
    }

    async fn get_last_seen_network_head(&mut self) -> Result<JsValue> {
        match self.node.get_network_head_header().await? {
            Some(header) => to_value(&header).context("could not serialise head header"),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    async fn get_sampling_metadata(&mut self, height: u64) -> Result<Option<SamplingMetadata>> {
        Ok(self.node.get_sampling_metadata(height).await?)
    }

    async fn request_all_blobs(
        &mut self,
        header: JsValue,
        namespace: Namespace,
        timeout_secs: Option<f64>,
    ) -> Result<Vec<Blob>> {
        let header = from_value(header)?;
        let timeout = timeout_secs.map(Duration::from_secs_f64);
        Ok(self
            .node
            .request_all_blobs(&header, namespace, timeout)
            .await?)
    }

    async fn process_command(&mut self, command: NodeCommand) -> WorkerResponse {
        match command {
            NodeCommand::IsRunning => WorkerResponse::IsRunning(true),
            NodeCommand::StartNode(_) => {
                WorkerResponse::NodeStarted(Err(Error::new("Node already started")))
            }
            NodeCommand::StopNode => unreachable!("StopNode is handled in `run()`"),
            NodeCommand::GetLocalPeerId => {
                WorkerResponse::LocalPeerId(self.node.local_peer_id().to_string())
            }
            NodeCommand::GetEventsChannelName => {
                WorkerResponse::EventsChannelName(self.events_channel_name.clone())
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
                WorkerResponse::LastSeenNetworkHead(self.get_last_seen_network_head().await.into())
            }
            NodeCommand::GetSamplingMetadata { height } => {
                WorkerResponse::SamplingMetadata(self.get_sampling_metadata(height).await)
            }
            NodeCommand::RequestAllBlobs {
                header,
                namespace,
                timeout_secs,
            } => WorkerResponse::Blobs(
                self.request_all_blobs(header, namespace, timeout_secs)
                    .await,
            ),
            NodeCommand::InternalPing => WorkerResponse::InternalPong,
        }
    }
}

async fn event_forwarder_task(mut events_sub: EventSubscriber, events_channel: BroadcastChannel) {
    #[derive(Serialize)]
    struct Event {
        message: String,
        is_error: bool,
        #[serde(flatten)]
        info: NodeEventInfo,
    }

    while let Ok(ev) = events_sub.recv().await {
        let ev = Event {
            message: ev.event.to_string(),
            is_error: ev.event.is_error(),
            info: ev,
        };

        if let Ok(val) = to_value(&ev) {
            if events_channel.post_message(&val).is_err() {
                break;
            }
        }
    }
}
