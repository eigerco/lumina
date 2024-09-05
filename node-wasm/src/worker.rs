use std::fmt::Debug;

use tokio::sync::{mpsc, Mutex};
use js_sys::Array;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tracing::{error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{BroadcastChannel, SharedWorker};

use lumina_node::events::{EventSubscriber, NodeEventInfo};
use lumina_node::node::{Node, SyncingInfo};
use lumina_node::store::{IndexedDbStore, SamplingMetadata, Store};

use crate::error::{Context, Error, Result};
use crate::node::WasmNodeConfig;
use crate::ports::{ClientId, MessagePortLike, RequestServer};
use crate::utils::{random_id, WorkerSelf};
use crate::worker::commands::{NodeCommand, SingleHeaderQuery, WorkerResponse};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

pub(crate) mod commands;

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

struct NodeWorker {
    node: Node<IndexedDbStore>,
    events_channel_name: String,
}

impl NodeWorker {
    async fn new(events_channel_name: &str, config: WasmNodeConfig) -> Result<Self> {
        let config = config.into_node_config().await?;

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialised new empty store");
        }

        let (node, events_sub) = Node::new_subscribed(config).await?;

        let events_channel = BroadcastChannel::new(events_channel_name)
            .context("Failed to allocate BroadcastChannel")?;

        spawn_local(event_forwarder_task(events_sub, events_channel));

        Ok(Self {
            node,
            events_channel_name: events_channel_name.to_owned(),
        })
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

    async fn process_command(&mut self, command: NodeCommand) -> WorkerResponse {
        match command {
            NodeCommand::IsRunning => WorkerResponse::IsRunning(true),
            NodeCommand::StartNode(_) => {
                WorkerResponse::NodeStarted(Err(Error::new("Node already started")))
            }
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
            NodeCommand::CloseWorker => {
                SharedWorker::worker_self().close();
                WorkerResponse::WorkerClosed(())
            }
        }
    }
}

#[wasm_bindgen]
struct NodeWorkerWrapper {
    event_channel_name: String,
    worker: Mutex<Option<NodeWorker>>,
    request_server: Mutex<RequestServer>,
    control_channel: mpsc::Sender<MessagePortLike>,
}

#[wasm_bindgen]
impl NodeWorkerWrapper {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        info!("Created lumina worker");

        // XXX: const
        let (tx, rx) = mpsc::channel(999);

        Self {
            event_channel_name: format!("NodeEventChannel-{}", random_id()),
            worker: Mutex::new(None),
            request_server: Mutex::new(RequestServer::new(rx)),
            control_channel: tx,
        }
    }

    pub async fn connect(&self, port: MessagePortLike) {
        self.control_channel.send(port).await.expect("RequestServer command channel should never close")
    }

    pub async fn poll(&self) -> Result<(), Error> {
        let (client_id, command) = self.next_command().await?;

        let mut worker_lock = self.worker.lock().await;
        let response = match &mut *worker_lock {
            Some(worker) => worker.process_command(command).await,
            worker @ None => match command {
                NodeCommand::IsRunning => WorkerResponse::IsRunning(false),
                NodeCommand::GetEventsChannelName => {
                    WorkerResponse::EventsChannelName(self.event_channel_name.clone())
                }
                NodeCommand::StartNode(config) => {
                    match NodeWorker::new(&self.event_channel_name, config).await {
                        Ok(node) => {
                            let _ = worker.insert(node);
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

        let server = self.request_server.lock().await;
        server.respond_to(client_id, response);

        Ok(())
    }

    async fn next_command(&self) -> Result<(ClientId, NodeCommand), Error> {
        let mut server = self.request_server.lock().await;
        let (client_id, result) = server.recv().await;

        let command = result
            .with_context(|| format!("could not parse command received from {client_id:?}"))?;

        Ok((client_id, command))
    }
}

/*
#[wasm_bindgen]
pub async fn run_worker(port: MessagePortLike) -> Result<()> {
    info!("Entered run_worker");
    let events_channel_name = format!("NodeEventChannel-{}", random_id());

    let mut request_server = RequestServer::new(todo!());
    //request_server.connect(port);

    info!("Entering worker message loop");
    let mut worker = None;
    loop {
        let (client_id, command_result) = request_server.recv().await;
        let command = match command_result {
            Ok(v) => v,
            Err(e) => {
                error!("Received invalid command from {client_id:?}: {e}");
                continue;
            }
        };

        debug!("received from {client_id:?}: {command:?}");

        let Some(worker) = &mut worker else {
            match command {
                NodeCommand::IsRunning => {
                    request_server.respond_to(client_id, WorkerResponse::IsRunning(false));
                }
                NodeCommand::GetEventsChannelName => {
                    request_server.respond_to(
                        client_id,
                        WorkerResponse::EventsChannelName(events_channel_name.clone()),
                    );
                }
                NodeCommand::StartNode(config) => {
                    match NodeWorker::new(&events_channel_name, config).await {
                        Ok(node) => {
                            worker = Some(node);
                            request_server
                                .respond_to(client_id, WorkerResponse::NodeStarted(Ok(())));
                        }
                        Err(e) => {
                            request_server
                                .respond_to(client_id, WorkerResponse::NodeStarted(Err(e)));
                        }
                    };
                }
                _ => {
                    warn!("Worker not running");
                    request_server.respond_to(client_id, WorkerResponse::NodeNotRunning);
                }
            }
            continue;
        };

        let response = worker.process_command(command).await;
        request_server.respond_to(client_id, response);
    }
}
*/

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

    events_channel.close();
}
