use serde::{Deserialize, Serialize};
use tracing::{info, trace, warn};
use wasm_bindgen::prelude::*;

use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use instant::Instant;
use js_sys::Array;
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::mpsc;
use wasm_bindgen_futures::spawn_local;
use web_sys::MessageEvent;
use web_sys::MessagePort;
use web_sys::SharedWorker;

use lumina_node::node::Node;
use lumina_node::store::{IndexedDbStore, Store};

use crate::node::WasmNodeConfig;
use crate::utils::js_value_from_display;
use crate::utils::BChannel;
use crate::utils::WorkerSelf;
use crate::wrapper::libp2p::NetworkInfoSnapshot;
use crate::Result;

#[derive(Serialize, Deserialize, Debug)]
pub enum NodeCommand {
    IsRunning,
    Start(WasmNodeConfig),
    GetLocalPeerId,
    GetSyncerInfo,
    GetPeerTrackerInfo,
    GetNetworkInfo,
    GetConnectedPeers,
    GetNetworkHeadHeader,
    GetLocalHeadHeader,
    SetPeerTrust { peer_id: String, is_trusted: bool },
    RequestHeadHeader,
    WaitConnected(bool),
    GetListeners,
    RequestHeader(HeaderQuery),
    GetHeader(HeaderQuery),
    GetSamplingMetadata(u64),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum HeaderQuery {
    ByHash(Hash),
    ByHeight(u64),
    GetVerified {
        #[serde(with = "serde_wasm_bindgen::preserve")]
        from: JsValue,
        amount: u64,
    },
    Range {
        start_height: Option<u64>,
        end_height: Option<u64>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NodeResponse {
    Running(bool),
    Started(u64),
    LocalPeerId(String),
    Connected(bool),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    SyncerInfo(JsValue),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    PeerTrackerInfo(JsValue),
    NetworkInfo(NetworkInfoSnapshot),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    ConnectedPeers(Array),
    PeerTrust {
        peer_id: String,
        is_trusted: bool,
    },
    #[serde(with = "serde_wasm_bindgen::preserve")]
    Header(JsValue),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    HeaderArray(Array),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    VerifiedHeaders(Array),
    #[serde(with = "serde_wasm_bindgen::preserve")]
    SamplingMetadata(JsValue),
}

struct NodeWorker {
    node: Node<IndexedDbStore>,
    start_timestamp: Instant,
}

impl NodeWorker {
    async fn new(config: WasmNodeConfig) -> Self {
        let config = config.into_node_config().await.ok().unwrap();

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialized new empty store");
        }

        let node = Node::new(config).await.ok().unwrap();

        Self {
            node,
            start_timestamp: Instant::now(),
        }
    }

    fn local_peer_id(&self) -> String {
        self.node.local_peer_id().to_string()
    }

    fn peer_tracker_info(&self) -> Result<JsValue> {
        Ok(to_value(&self.node.peer_tracker_info())?)
    }

    async fn syncer_info(&self) -> Result<JsValue> {
        Ok(to_value(&self.node.syncer_info().await?)?)
    }

    async fn network_info(&self) -> Result<NetworkInfoSnapshot> {
        Ok(self.node.network_info().await?.into())
    }

    async fn request_header_by_hash(&self, hash: Hash) -> Result<JsValue> {
        Ok(to_value(&self.node.request_header_by_hash(&hash).await?)?)
    }

    async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        Ok(to_value(
            &self.node.request_header_by_height(height).await?,
        )?)
    }

    async fn get_header_by_hash(&self, hash: Hash) -> Result<JsValue> {
        Ok(to_value(&self.node.get_header_by_hash(&hash).await?)?)
    }

    async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        Ok(to_value(&self.node.get_header_by_height(height).await?)?)
    }

    async fn get_headers(
        &self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Array> {
        let headers = match (start_height, end_height) {
            (None, None) => self.node.get_headers(..).await,
            (Some(start), None) => self.node.get_headers(start..).await,
            (None, Some(end)) => self.node.get_headers(..=end).await,
            (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
        }?;

        Ok(to_value(&headers)?.into())
    }

    async fn request_verified_headers(&self, from: ExtendedHeader, amount: u64) -> Result<Array> {
        Ok(to_value(&self.node.request_verified_headers(&from, amount).await?)?.into())
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        Ok(to_value(&self.node.get_sampling_metadata(height).await?)?)
    }

    async fn set_peer_trust(&self, peer_id: String, is_trusted: bool) -> Result<()> {
        Ok(self
            .node
            .set_peer_trust(peer_id.parse()?, is_trusted)
            .await?)
    }

    async fn connected_peers(&self) -> Result<Array> {
        Ok(self
            .node
            .connected_peers()
            .await?
            .iter()
            .map(js_value_from_display)
            .collect())
    }

    async fn network_head_header(&self) -> Result<JsValue> {
        Ok(to_value(&self.node.get_network_head_header())?)
    }

    async fn wait_connected(&self, trusted: bool) {
        if trusted {
            self.node.wait_connected().await;
        } else {
            self.node.wait_connected_trusted().await;
        }
    }

    async fn local_head_header(&self) -> Result<JsValue> {
        Ok(to_value(&self.node.get_local_head_header().await?)?)
    }

    async fn request_head_header(&self) -> Result<JsValue> {
        Ok(to_value(&self.node.request_head_header().await?)?)
    }
    async fn process_command(&mut self, command: NodeCommand) -> NodeResponse {
        match command {
            // TODO: order
            NodeCommand::IsRunning => NodeResponse::Running(true),
            NodeCommand::Start(_config) => NodeResponse::Started(
                Instant::now()
                    .checked_duration_since(self.start_timestamp)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0),
            ),
            NodeCommand::GetLocalPeerId => NodeResponse::LocalPeerId(self.local_peer_id()),
            NodeCommand::GetSyncerInfo => {
                NodeResponse::SyncerInfo(self.syncer_info().await.ok().unwrap())
            }
            NodeCommand::GetPeerTrackerInfo => {
                NodeResponse::PeerTrackerInfo(self.peer_tracker_info().ok().unwrap())
            }
            NodeCommand::GetNetworkInfo => {
                NodeResponse::NetworkInfo(self.network_info().await.ok().unwrap())
            }
            NodeCommand::GetConnectedPeers => {
                NodeResponse::ConnectedPeers(self.connected_peers().await.ok().unwrap())
            }
            NodeCommand::GetNetworkHeadHeader => {
                NodeResponse::Header(self.network_head_header().await.ok().unwrap())
            }
            NodeCommand::GetLocalHeadHeader => {
                NodeResponse::Header(self.local_head_header().await.ok().unwrap())
            }
            NodeCommand::SetPeerTrust {
                peer_id,
                is_trusted,
            } => {
                //XXX
                self.set_peer_trust(peer_id.clone(), is_trusted)
                    .await
                    .ok()
                    .unwrap();
                NodeResponse::PeerTrust {
                    peer_id,
                    is_trusted,
                }
            }
            NodeCommand::RequestHeadHeader => {
                NodeResponse::Header(self.request_head_header().await.ok().unwrap())
            }
            NodeCommand::WaitConnected(trusted) => {
                todo!()
            }
            NodeCommand::GetListeners => {
                todo!()
            }
            NodeCommand::RequestHeader(HeaderQuery::ByHash(hash)) => {
                NodeResponse::Header(self.request_header_by_hash(hash).await.ok().unwrap())
            }
            NodeCommand::RequestHeader(HeaderQuery::ByHeight(height)) => {
                NodeResponse::Header(self.request_header_by_height(height).await.ok().unwrap())
            }
            NodeCommand::GetHeader(HeaderQuery::ByHash(hash)) => {
                NodeResponse::Header(self.get_header_by_hash(hash).await.ok().unwrap())
            }
            NodeCommand::GetHeader(HeaderQuery::ByHeight(height)) => {
                NodeResponse::Header(self.get_header_by_height(height).await.ok().unwrap())
            }
            NodeCommand::GetSamplingMetadata(height) => NodeResponse::SamplingMetadata(
                self.get_sampling_metadata(height).await.ok().unwrap(),
            ),
            NodeCommand::RequestHeader(HeaderQuery::GetVerified { from, amount }) => {
                NodeResponse::VerifiedHeaders(
                    self.request_verified_headers(from_value(from).ok().unwrap(), amount)
                        .await
                        .ok()
                        .unwrap(),
                )
            }
            NodeCommand::GetHeader(HeaderQuery::GetVerified { .. }) => unimplemented!(),
            NodeCommand::RequestHeader(HeaderQuery::Range { .. }) => unimplemented!(),
            NodeCommand::GetHeader(HeaderQuery::Range {
                start_height,
                end_height,
            }) => NodeResponse::HeaderArray(
                self.get_headers(start_height, end_height)
                    .await
                    .ok()
                    .unwrap(),
            ),
            NodeCommand::WaitConnected(trusted) => {
                self.wait_connected(trusted).await; // TODO: nonblocking ( Promise? )
                NodeResponse::Connected(trusted)
            }
        }
    }
}

//type WorkerChannel = BChannel<NodeCommand, NodeResponse>;

enum WorkerMessage {
    NewConnection(MessagePort),
    Command((NodeCommand, ClientId)),
}

#[derive(Debug)]
struct ClientId(usize);

struct WorkerConnector {
    // make sure the callback doesn't get dropped
    _onconnect_callback: Closure<dyn Fn(MessageEvent)>,
    callbacks: Vec<Closure<dyn Fn(MessageEvent)>>,
    ports: Vec<MessagePort>,

    command_channel: mpsc::Sender<WorkerMessage>,
}

impl WorkerConnector {
    fn new(command_channel: mpsc::Sender<WorkerMessage>) -> Self {
        let worker_scope = SharedWorker::worker_self();
        let near_tx = command_channel.clone();
        let onconnect_callback: Closure<dyn Fn(MessageEvent)> =
            Closure::new(move |ev: MessageEvent| {
                let local_tx = near_tx.clone();
                spawn_local(async move {
                    let port: MessagePort = ev.ports().at(0).dyn_into().expect("invalid type");
                    local_tx
                        .send(WorkerMessage::NewConnection(port))
                        .await
                        .expect("send2 error");
                })
            });
        worker_scope.set_onconnect(Some(onconnect_callback.as_ref().unchecked_ref()));

        Self {
            _onconnect_callback: onconnect_callback,
            callbacks: Vec::new(),
            ports: Vec::new(),
            command_channel,
        }
    }

    fn add(&mut self, port: MessagePort) {
        debug_assert_eq!(self.callbacks.len(), self.ports.len());
        let client_id = self.callbacks.len();

        let near_tx = self.command_channel.clone();
        let client_message_callback: Closure<dyn Fn(MessageEvent)> =
            Closure::new(move |ev: MessageEvent| {
                let local_tx = near_tx.clone();
                spawn_local(async move {
                    let message_data = ev.data();
                    let data = from_value(message_data).expect("could not from value");

                    local_tx
                        .send(WorkerMessage::Command((data, ClientId(client_id))))
                        .await
                        .expect("send3 err");
                })
            });
        port.set_onmessage(Some(client_message_callback.as_ref().unchecked_ref()));

        self.callbacks.push(client_message_callback);
        self.ports.push(port);

        info!("New connection: {client_id}");
    }

    fn respond_to(&self, client: ClientId, msg: NodeResponse) {
        let off = client.0;
        let v = to_value(&msg).expect("could not to_value");
        self.ports[off].post_message(&v).expect("err posttt");
    }
}

#[wasm_bindgen]
pub async fn run_worker(queued_connections: Vec<MessagePort>) {
    let (tx, mut rx) = mpsc::channel(64);
    let mut connector = WorkerConnector::new(tx.clone());

    for connection in queued_connections {
        connector.add(connection);
    }

    let mut worker = None;
    while let Some(message) = rx.recv().await {
        match message {
            WorkerMessage::NewConnection(connection) => {
                connector.add(connection);
            }
            WorkerMessage::Command((command, client)) => {
                let Some(worker) = &mut worker else {
                    match command {
                        NodeCommand::IsRunning => {
                            connector.respond_to(client, NodeResponse::Running(false));
                        }
                        NodeCommand::Start(config) => {
                            worker = Some(NodeWorker::new(config).await);
                            connector.respond_to(client, NodeResponse::Started(0));
                        }
                        _ => warn!("Worker not running"),
                    }
                    continue;
                };

                trace!("received: {command:?}");
                let response = worker.process_command(command).await;
                connector.respond_to(client, response);
            }
        }
    }

    warn!("EXIT EXIT EXIT");
}
