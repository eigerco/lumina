use std::fmt::Debug;

use js_sys::Array;
use libp2p::{Multiaddr, PeerId};
use lumina_node::events::{EventSubscriber, NodeEventInfo};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{BroadcastChannel, MessageEvent, SharedWorker};

use lumina_node::node::{Node, SyncingInfo};
use lumina_node::store::{IndexedDbStore, SamplingMetadata, Store};

use crate::error::{Context, Error, Result};
use crate::node::WasmNodeConfig;
use crate::utils::{get_crypto, WorkerSelf};
use crate::worker::channel::{
    DedicatedWorkerMessageServer, MessageServer, SharedWorkerMessageServer, WorkerMessage,
};
use crate::worker::commands::{NodeCommand, SingleHeaderQuery, WorkerResponse};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

mod channel;
pub(crate) mod commands;

pub(crate) use channel::{AnyWorker, WorkerClient};

const WORKER_MESSAGE_SERVER_INCOMING_QUEUE_LENGTH: usize = 64;

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
pub async fn run_worker(queued_events: Vec<MessageEvent>) -> Result<()> {
    info!("Entered run_worker");
    let (tx, mut rx) = mpsc::channel(WORKER_MESSAGE_SERVER_INCOMING_QUEUE_LENGTH);
    let events_channel_name = format!("NodeEventChannel-{}", get_crypto()?.random_uuid());

    let mut message_server: Box<dyn MessageServer> = if SharedWorker::is_worker_type() {
        Box::new(SharedWorkerMessageServer::new(tx.clone(), queued_events))
    } else {
        Box::new(DedicatedWorkerMessageServer::new(tx.clone(), queued_events).await)
    };

    info!("Entering worker message loop");
    let mut worker = None;
    while let Some(message) = rx.recv().await {
        match message {
            WorkerMessage::NewConnection(connection) => {
                message_server.add(connection);
            }
            WorkerMessage::InvalidCommandReceived(client_id) => {
                message_server.respond_err_to(client_id, WorkerError::InvalidCommandReceived);
            }
            WorkerMessage::Command((command, client_id)) => {
                debug!("received from {client_id:?}: {command:?}");
                let Some(worker) = &mut worker else {
                    match command {
                        NodeCommand::IsRunning => {
                            message_server.respond_to(client_id, WorkerResponse::IsRunning(false));
                        }
                        NodeCommand::GetEventsChannelName => {
                            message_server.respond_to(
                                client_id,
                                WorkerResponse::EventsChannelName(events_channel_name.clone()),
                            );
                        }
                        NodeCommand::StartNode(config) => {
                            match NodeWorker::new(&events_channel_name, config).await {
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

    info!("Channel to WorkerMessageServer closed, exiting the SharedWorker");

    Ok(())
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

    events_channel.close();
}
