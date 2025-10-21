use std::fmt::Debug;
use std::time::Duration;

use blockstore::EitherBlockstore;
use celestia_types::nmt::Namespace;
use celestia_types::Blob;
use futures::StreamExt;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{BroadcastChannel, MessageChannel, MessageEvent, MessagePort};

use celestia_types::ExtendedHeader;
use lumina_node::blockstore::{InMemoryBlockstore, IndexedDbBlockstore};
use lumina_node::events::{EventSubscriber, NodeEventInfo};
use lumina_node::node::SubscriptionError;
use lumina_node::node::{Node, SyncingInfo};
use lumina_node::store::{EitherStore, InMemoryStore, IndexedDbStore, SamplingMetadata};
use lumina_utils::executor::spawn;

use crate::client::WasmNodeConfig;
use crate::commands::{
    Command, CommandWithResponder, ManagementCommand, NodeCommand, NodeSubscription,
    SingleHeaderQuery, WorkerError, WorkerResponse, WorkerResult,
};
use crate::error::{Context, Error, Result};
use crate::ports::Port;
use crate::utils::random_id;
use crate::worker_server::WorkerServer;
use crate::wrapper::libp2p::NetworkInfoSnapshot;

pub(crate) type WasmBlockstore = EitherBlockstore<InMemoryBlockstore, IndexedDbBlockstore>;
pub(crate) type WasmStore = EitherStore<InMemoryStore, IndexedDbStore>;

/// `NodeWorker` is responsible for receiving commands from connected [`NodeClient`]s, executing
/// them and sending a response back, as well as accepting new `NodeClient` connections.
///
/// [`NodeClient`]: crate::client::NodeClient
#[wasm_bindgen]
pub struct NodeWorker {
    event_channel_name: String,
    node: Option<NodeWorkerInstance>,
    request_server: WorkerServer,
}

struct NodeWorkerInstance {
    node: Node<WasmBlockstore, WasmStore>,
}

#[wasm_bindgen]
impl NodeWorker {
    #[wasm_bindgen(constructor)]
    pub fn new(port_like_object: JsValue) -> Self {
        info!("Created lumina worker");

        let request_server = WorkerServer::new();
        let port_channel = request_server.get_port_channel();

        port_channel
            .send(port_like_object)
            .expect("control channel should be ready to receive now");

        Self {
            event_channel_name: format!("NodeEventChannel-{}", random_id()),
            node: None,
            request_server,
        }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        loop {
            let CommandWithResponder { command, responder } = self.request_server.recv().await?;

            let response = self.execute_command(command).await;

            if responder.send(response).is_err() {
                error!("Failed to send response: channel dropped");
            }
        }
    }

    async fn execute_command(&mut self, command: Command) -> Result<WorkerResponse, WorkerError> {
        Ok(match command {
            Command::Management(command) => match command {
                ManagementCommand::InternalPing => WorkerResponse::InternalPong,
                ManagementCommand::IsRunning => WorkerResponse::IsRunning(self.node.is_some()),
                ManagementCommand::GetEventsChannelName => {
                    WorkerResponse::EventsChannelName(self.event_channel_name.clone())
                }
                ManagementCommand::StartNode(config) => match &mut self.node {
                    Some(_) => return Err(WorkerError::NodeAlreadyRunning),
                    node => {
                        let instance =
                            NodeWorkerInstance::new(&self.event_channel_name, config).await?;
                        let _ = node.insert(instance);
                        WorkerResponse::Ok
                    }
                },
                ManagementCommand::StopNode => {
                    let node = self.node.take().ok_or(WorkerError::NodeNotRunning)?;
                    node.stop().await;
                    WorkerResponse::Ok
                }
                ManagementCommand::ConnectPort(port) => {
                    self.request_server.spawn_connection_worker(
                        port.ok_or(WorkerError::InvalidCommandReceived)?,
                    )?;
                    WorkerResponse::Ok
                }
            },
            Command::Node(command) => {
                self.node
                    .as_mut()
                    .ok_or(WorkerError::NodeNotRunning)?
                    .process_command(command)
                    .await?
            }
            Command::Subscribe(command) => {
                let port = self
                    .node
                    .as_mut()
                    .ok_or(WorkerError::NodeNotRunning)?
                    .process_subscription(command)
                    .await?;
                WorkerResponse::Subscribed(Some(port.into()))
            }
        })
    }
}

impl NodeWorkerInstance {
    async fn new(events_channel_name: &str, config: WasmNodeConfig) -> Result<Self> {
        let builder = config.into_node_builder().await?;
        let (node, events_sub) = builder.start_subscribed().await?;

        let events_channel = BroadcastChannel::new(events_channel_name)
            .context("Failed to allocate BroadcastChannel")?;

        spawn_local(event_forwarder_task(events_sub, events_channel));

        Ok(Self { node })
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
        from: ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(self.node.request_verified_headers(&from, amount).await?)
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

    async fn get_last_seen_network_head(&mut self) -> Result<Option<ExtendedHeader>> {
        Ok(self.node.get_network_head_header().await?)
    }

    async fn get_sampling_metadata(&mut self, height: u64) -> Result<Option<SamplingMetadata>> {
        Ok(self.node.get_sampling_metadata(height).await?)
    }

    async fn request_all_blobs(
        &mut self,
        namespace: Namespace,
        block_height: u64,
        timeout_secs: Option<f64>,
    ) -> Result<Vec<Blob>> {
        let timeout = timeout_secs.map(Duration::from_secs_f64);
        Ok(self
            .node
            .request_all_blobs(namespace, block_height, timeout)
            .await?)
    }

    async fn process_command(&mut self, command: NodeCommand) -> WorkerResult {
        Ok(match command {
            NodeCommand::GetLocalPeerId => {
                WorkerResponse::LocalPeerId(self.node.local_peer_id().to_string())
            }
            NodeCommand::GetSyncerInfo => WorkerResponse::SyncerInfo(self.get_syncer_info().await?),
            NodeCommand::GetPeerTrackerInfo => {
                let peer_tracker_info = self.node.peer_tracker_info();
                WorkerResponse::PeerTrackerInfo(peer_tracker_info)
            }
            NodeCommand::GetNetworkInfo => {
                WorkerResponse::NetworkInfo(self.get_network_info().await?)
            }
            NodeCommand::GetConnectedPeers => {
                WorkerResponse::ConnectedPeers(self.get_connected_peers().await?)
            }
            NodeCommand::SetPeerTrust {
                peer_id,
                is_trusted,
            } => {
                self.set_peer_trust(peer_id, is_trusted).await?;
                WorkerResponse::Ok
            }
            NodeCommand::WaitConnected { trusted } => {
                self.wait_connected(trusted).await?;
                WorkerResponse::Ok
            }
            NodeCommand::GetListeners => WorkerResponse::Listeners(self.get_listeners().await?),
            NodeCommand::RequestHeader(query) => {
                WorkerResponse::Header(self.request_header(query).await?)
            }
            NodeCommand::GetHeader(query) => WorkerResponse::Header(self.get_header(query).await?),
            NodeCommand::GetVerifiedHeaders { from, amount } => {
                WorkerResponse::Headers(self.get_verified_headers(from, amount).await?)
            }
            NodeCommand::GetHeadersRange {
                start_height,
                end_height,
            } => WorkerResponse::Headers(self.get_headers_range(start_height, end_height).await?),
            NodeCommand::LastSeenNetworkHead => {
                WorkerResponse::LastSeenNetworkHead(self.get_last_seen_network_head().await?)
            }
            NodeCommand::GetSamplingMetadata { height } => {
                WorkerResponse::SamplingMetadata(self.get_sampling_metadata(height).await?)
            }
            NodeCommand::RequestAllBlobs {
                namespace,
                block_height,
                timeout_secs,
            } => WorkerResponse::Blobs(
                self.request_all_blobs(namespace, block_height, timeout_secs)
                    .await?,
            ),
        })
    }

    async fn process_subscription(
        &mut self,
        subscription: NodeSubscription,
    ) -> Result<MessagePort> {
        match subscription {
            NodeSubscription::Headers => {
                let stream = self.node.header_subscribe().await?;
                register_forwarding_tasks_and_callbacks(stream)
            }
            NodeSubscription::Blobs(namespace) => {
                let stream = self.node.namespace_subscribe(namespace).await?;
                register_forwarding_tasks_and_callbacks(stream)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SubscriptionFeedback {
    /// receiver has read the previous item and is ready for more
    Ready,
    /// receiver closed the channel
    Close,
}

fn register_forwarding_tasks_and_callbacks<T: Serialize + 'static>(
    mut stream: ReceiverStream<Result<T, SubscriptionError>>,
) -> Result<MessagePort> {
    let channel = MessageChannel::new()?;
    let (signal_tx, mut signal_rx) = mpsc::channel(1);

    let port = Port::new(
        channel.port1().into(),
        move |ev: MessageEvent| -> Result<()> {
            let signal: SubscriptionFeedback =
                from_value(ev.data()).context("could not deserialize subscription signal")?;
            if signal_tx.try_send(signal).is_err() {
                error!("Error forwarding subscription signal, should not happen");
            }

            Ok(())
        },
    )?;

    spawn(async move {
        info!("Starting subscription");
        loop {
            match signal_rx.recv().await {
                Some(SubscriptionFeedback::Ready) => (),
                Some(SubscriptionFeedback::Close) => break,
                None => {
                    warn!("unexpected subscription signal channel close, should not happen");
                    break;
                }
            }
            let item: Result<Option<T>> = stream.next().await.transpose().map_err(Error::from);

            if let Err(e) = port.send(&item) {
                error!("Error sending subscription item: {e}");
            }
        }
        info!("Ending subscription");
    });

    Ok(channel.port2())
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
