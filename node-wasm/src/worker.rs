use std::fmt::Debug;

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
use lumina_node::syncer::SyncingInfo;

use crate::node::WasmNodeConfig;
use crate::utils::js_value_from_display;
use crate::utils::CommandResponseChannel;
use crate::utils::NodeCommandResponse;
use crate::utils::NodeCommandType;
use crate::utils::WorkerSelf;
//use crate::worker::NodeResponse;
use crate::wrapper::libp2p::NetworkInfoSnapshot;
use crate::Result;
use libp2p::Multiaddr;
use lumina_node::peer_tracker::PeerTrackerInfo;
use lumina_node::store::SamplingMetadata;
use tokio::sync::oneshot;

#[derive(Debug, Serialize, Deserialize)]
pub struct IsRunning;
impl NodeCommandType for IsRunning {
    type Output = bool;
}
impl From<IsRunning> for NodeCommand {
    fn from(command: IsRunning) -> NodeCommand {
        NodeCommand::IsRunning(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum NodeState {
    NodeStopped,
    NodeStarted,
    AlreadyRunning(u64),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartNode(pub WasmNodeConfig);
impl NodeCommandType for StartNode {
    type Output = NodeState;
}
impl From<StartNode> for NodeCommand {
    fn from(command: StartNode) -> NodeCommand {
        NodeCommand::StartNode(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetLocalPeerId;
impl NodeCommandType for GetLocalPeerId {
    type Output = String;
}
impl From<GetLocalPeerId> for NodeCommand {
    fn from(command: GetLocalPeerId) -> NodeCommand {
        NodeCommand::GetLocalPeerId(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetSyncerInfo;
impl NodeCommandType for GetSyncerInfo {
    type Output = SyncingInfo;
}
impl From<GetSyncerInfo> for NodeCommand {
    fn from(command: GetSyncerInfo) -> NodeCommand {
        NodeCommand::GetSyncerInfo(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPeerTrackerInfo;
impl NodeCommandType for GetPeerTrackerInfo {
    type Output = PeerTrackerInfo;
}
impl From<GetPeerTrackerInfo> for NodeCommand {
    fn from(command: GetPeerTrackerInfo) -> NodeCommand {
        NodeCommand::GetPeerTrackerInfo(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetNetworkInfo;
impl NodeCommandType for GetNetworkInfo {
    type Output = NetworkInfoSnapshot;
}
impl From<GetNetworkInfo> for NodeCommand {
    fn from(command: GetNetworkInfo) -> NodeCommand {
        NodeCommand::GetNetworkInfo(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetConnectedPeers;
impl NodeCommandType for GetConnectedPeers {
    type Output = Vec<String>;
}
impl From<GetConnectedPeers> for NodeCommand {
    fn from(command: GetConnectedPeers) -> NodeCommand {
        NodeCommand::GetConnectedPeers(command)
    }
}

/*
#[derive(Debug, Serialize, Deserialize)]
pub struct GetNetworkHeadHeader;
impl NodeCommandType for GetNetworkHeadHeader {
    type Output = JsValue;
}
impl From<GetNetworkHeadHeader> for NodeCommand {
    fn from(command: GetNetworkHeadHeader) -> NodeCommand {
        NodeCommand::GetNetworkHeadHeader(command)
    }
}
*/

/*
#[derive(Debug, Serialize, Deserialize)]
pub struct GetLocalHeadHeader;
impl NodeCommandType for GetLocalHeadHeader {
    type Output = JsValue;
}
impl From<GetLocalHeadHeader> for NodeCommand {
    fn from(command: GetLocalHeadHeader) -> NodeCommand {
        NodeCommand::GetLocalHeadHeader(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestHeadHeader;
impl NodeCommandType for RequestHeadHeader {
    type Output = JsValue;
}
impl From<RequestHeadHeader> for NodeCommand {
    fn from(command: RequestHeadHeader) -> NodeCommand {
        NodeCommand::RequestHeadHeader(command)
    }
}
*/

#[derive(Debug, Serialize, Deserialize)]
pub struct SetPeerTrust {
    peer_id: String,
    is_trusted: bool,
}
impl NodeCommandType for SetPeerTrust {
    type Output = ();
}
impl From<SetPeerTrust> for NodeCommand {
    fn from(command: SetPeerTrust) -> NodeCommand {
        NodeCommand::SetPeerTrust(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WaitConnected {
    trusted: bool,
}
impl NodeCommandType for WaitConnected {
    type Output = ();
}
impl From<WaitConnected> for NodeCommand {
    fn from(command: WaitConnected) -> NodeCommand {
        NodeCommand::WaitConnected(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetListeners;
impl NodeCommandType for GetListeners {
    type Output = Vec<Multiaddr>;
}
impl From<GetListeners> for NodeCommand {
    fn from(command: GetListeners) -> NodeCommand {
        NodeCommand::GetListeners(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestHeader(pub SingleHeaderQuery);
impl NodeCommandType for RequestHeader {
    type Output = ExtendedHeader;
}
impl From<RequestHeader> for NodeCommand {
    fn from(command: RequestHeader) -> NodeCommand {
        NodeCommand::RequestHeader(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestMultipleHeaders(pub MultipleHeaderQuery);
impl NodeCommandType for RequestMultipleHeaders {
    type Output = Array;
}
impl From<RequestMultipleHeaders> for NodeCommand {
    fn from(command: RequestMultipleHeaders) -> NodeCommand {
        NodeCommand::RequestMultipleHeaders(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetHeader(pub SingleHeaderQuery);
impl NodeCommandType for GetHeader {
    type Output = ExtendedHeader;
}
impl From<GetHeader> for NodeCommand {
    fn from(command: GetHeader) -> NodeCommand {
        NodeCommand::GetHeader(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetMultipleHeaders(pub MultipleHeaderQuery);
impl NodeCommandType for GetMultipleHeaders {
    type Output = Array;
}
impl From<GetMultipleHeaders> for NodeCommand {
    fn from(command: GetMultipleHeaders) -> NodeCommand {
        NodeCommand::GetMultipleHeaders(command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetSamplingMetadata {
    pub height: u64,
}
impl NodeCommandType for GetSamplingMetadata {
    type Output = SamplingMetadata;
}
impl From<GetSamplingMetadata> for NodeCommand {
    fn from(command: GetSamplingMetadata) -> NodeCommand {
        NodeCommand::GetSamplingMetadata(command)
    }
}

// type being transmitted over the JS channel
#[derive(Serialize, Deserialize, Debug)]
pub enum NodeCommand {
    IsRunning(IsRunning),
    StartNode(StartNode),
    GetLocalPeerId(GetLocalPeerId),
    GetSyncerInfo(GetSyncerInfo),
    GetPeerTrackerInfo(GetPeerTrackerInfo),
    GetNetworkInfo(GetNetworkInfo),
    GetConnectedPeers(GetConnectedPeers),
    //GetNetworkHeadHeader,
    //GetLocalHeadHeader,
    SetPeerTrust(SetPeerTrust),
    //RequestHeadHeader(RequestHeadHeader),
    WaitConnected(WaitConnected),
    GetListeners(GetListeners),
    RequestHeader(RequestHeader),
    RequestMultipleHeaders(RequestMultipleHeaders),
    GetHeader(GetHeader),
    GetMultipleHeaders(GetMultipleHeaders),
    GetSamplingMetadata(GetSamplingMetadata),
}

#[derive(Debug)]
pub enum NodeCommandWithChannel {
    IsRunning((IsRunning, CommandResponseChannel<IsRunning>)),
    StartNode((StartNode, CommandResponseChannel<StartNode>)),
    GetLocalPeerId((GetLocalPeerId, CommandResponseChannel<GetLocalPeerId>)),
    GetSyncerInfo((GetSyncerInfo, CommandResponseChannel<GetSyncerInfo>)),
    GetPeerTrackerInfo(
        (
            GetPeerTrackerInfo,
            CommandResponseChannel<GetPeerTrackerInfo>,
        ),
    ),
    GetNetworkInfo((GetNetworkInfo, CommandResponseChannel<GetNetworkInfo>)),
    GetConnectedPeers((GetConnectedPeers, CommandResponseChannel<GetConnectedPeers>)),
    //GetNetworkHeadHeader,
    //GetLocalHeadHeader,
    SetPeerTrust((SetPeerTrust, CommandResponseChannel<SetPeerTrust>)),
    //RequestHeadHeader((RequestHeadHeader, CommandResponseChannel<RequestHeadHeader>)),
    WaitConnected((WaitConnected, CommandResponseChannel<WaitConnected>)),
    GetListeners((GetListeners, CommandResponseChannel<GetListeners>)),
    RequestHeader((RequestHeader, CommandResponseChannel<RequestHeader>)),
    RequestMultipleHeaders(
        (
            RequestMultipleHeaders,
            CommandResponseChannel<RequestMultipleHeaders>,
        ),
    ),
    GetHeader((GetHeader, CommandResponseChannel<GetHeader>)),
    GetMultipleHeaders(
        (
            GetMultipleHeaders,
            CommandResponseChannel<GetMultipleHeaders>,
        ),
    ),
    GetSamplingMetadata(
        (
            GetSamplingMetadata,
            CommandResponseChannel<GetSamplingMetadata>,
        ),
    ),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SingleHeaderQuery {
    Head,
    ByHash(Hash),
    ByHeight(u64),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MultipleHeaderQuery {
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
    IsRunning(NodeCommandResponse<IsRunning>),
    StartNode(NodeCommandResponse<StartNode>),
    GetLocalPeerId(NodeCommandResponse<GetLocalPeerId>),
    GetSyncerInfo(NodeCommandResponse<GetSyncerInfo>),
    GetPeerTrackerInfo(NodeCommandResponse<GetPeerTrackerInfo>),
    GetNetworkInfo(NodeCommandResponse<GetNetworkInfo>),
    GetConnectedPeers(NodeCommandResponse<GetConnectedPeers>),
    //GetNetworkHeadHeader,
    //GetLocalHeadHeader,
    SetPeerTrust(NodeCommandResponse<SetPeerTrust>),
    //RequestHeadHeader(NodeCommandResponse<RequestHeadHeader>),
    WaitConnected(NodeCommandResponse<WaitConnected>),
    GetListeners(NodeCommandResponse<GetListeners>),
    RequestHeader(NodeCommandResponse<RequestHeader>),
    GetHeader(NodeCommandResponse<GetHeader>),
    GetSamplingMetadata(NodeCommandResponse<GetSamplingMetadata>),
}

/*
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
    #[serde(with = "serde_wasm_bindgen::preserve")]
    Listeners(Array),
}
*/

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

    /*
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
    */
    async fn process_command(&mut self, command: NodeCommandWithChannel) {
        match command {
            // TODO: order
            NodeCommandWithChannel::IsRunning((_, response)) => {
                response.send(true).expect("channel_dropped")
            }
            NodeCommandWithChannel::StartNode((_, response)) => {
                response
                    .send(NodeState::AlreadyRunning(
                        Instant::now()
                            .checked_duration_since(self.start_timestamp)
                            .map(|duration| duration.as_secs())
                            .unwrap_or(0),
                    ))
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::GetLocalPeerId((_, response)) => {
                response
                    .send(self.node.local_peer_id().to_string())
                    .expect("channel_dropped");
            }
            NodeCommandWithChannel::GetSyncerInfo((_, response)) => {
                let syncer_info = self.node.syncer_info().await.unwrap();
                response.send(syncer_info).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetPeerTrackerInfo((_, response)) => {
                let peer_tracker_info = self.node.peer_tracker_info();
                response.send(peer_tracker_info).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetNetworkInfo((_, response)) => {
                let network_info = self.node.network_info().await.expect("TODO").into();
                response.send(network_info).expect("channel_dropped")
            }
            NodeCommandWithChannel::GetConnectedPeers((_, response)) => {
                let connected_peers = self.node.connected_peers().await.expect("TODO");
                /*
                                    .expect("TODO")
                                    .iter()
                                    .map(js_value_from_display)
                                    .collect();
                */
                response
                    .send(connected_peers.iter().map(|id| id.to_string()).collect())
                    .expect("channel_dropped");
            }
            /*
                        NodeCommandWithChannel::GetNetworkHeadHeader(command) => {
                            let _ = to_value(&self.node.get_network_head_header())
                                .ok()
                                .expect("TODO");
                            NodeResponse::Header(self.network_head_header().await.ok().unwrap())
                        }
                        NodeCommandWithChannel::GetLocalHeadHeader(command) => {
                            let _ = to_value(&self.node.get_local_head_header().await.unwrap())
                                .ok()
                                .unwrap();
                            NodeResponse::Header(self.local_head_header().await.ok().unwrap())
                        }
            */
            NodeCommandWithChannel::SetPeerTrust((
                SetPeerTrust {
                    peer_id,
                    is_trusted,
                },
                response,
            )) => {
                let _ = self
                    .node
                    .set_peer_trust(peer_id.parse().unwrap(), is_trusted)
                    .await
                    .unwrap();
                response.send(()).expect("channel_dropped");
            }
            NodeCommandWithChannel::WaitConnected((parameters, response)) => {
                // TODO: nonblocking on channels
                if parameters.trusted {
                    self.node.wait_connected().await;
                } else {
                    self.node.wait_connected_trusted().await;
                }
                response.send(()).expect("channel_dropped")
            }
            NodeCommandWithChannel::GetListeners((_command, _response)) => {
                todo!()
            }
            NodeCommandWithChannel::RequestHeader((command, response)) => {
                let header = match command.0 {
                    SingleHeaderQuery::Head => todo!(),
                    SingleHeaderQuery::ByHash(hash) => {
                        self.node.request_header_by_hash(&hash).await.ok().unwrap()
                    }
                    SingleHeaderQuery::ByHeight(height) => self
                        .node
                        .request_header_by_height(height)
                        .await
                        .ok()
                        .unwrap(),
                };
                //let jsvalue = to_value(&header).ok().unwrap();
                response.send(header).expect("channel_dropped");
            }
            NodeCommandWithChannel::RequestMultipleHeaders((command, response)) => {
                let headers = match command.0 {
                    MultipleHeaderQuery::GetVerified { from, amount } => {
                        let from_header = from_value(from).ok().unwrap();
                        self.node
                            .request_verified_headers(&from_header, amount)
                            .await
                            .unwrap()
                    }
                    MultipleHeaderQuery::Range {
                        start_height,
                        end_height,
                    } => todo!(), /*
                                  * match (start_height, end_height) {
                                  (None, None) => node.get_headers(..).await,
                                  (Some(start), None) => node.get_headers(start..).await,
                                  (None, Some(end)) => node.get_headers(..=end).await,
                                  (Some(start), Some(end)) => node.get_headers(start..=end).await,
                                  }
                                  .ok()
                                  .unwrap(),
                                  */
                };

                let jsvalue = to_value(&headers).ok().unwrap().into(); // TODO: array fix?
                response.send(jsvalue).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetHeader((command, response)) => {
                let header = match command.0 {
                    SingleHeaderQuery::Head => todo!(),
                    SingleHeaderQuery::ByHash(hash) => {
                        self.node.get_header_by_hash(&hash).await.ok().unwrap()
                    }
                    SingleHeaderQuery::ByHeight(height) => {
                        self.node.get_header_by_height(height).await.ok().unwrap()
                    }
                };
                //let jsvalue = to_value(&header).ok().unwrap();
                response.send(header).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetMultipleHeaders((command, response)) => {
                let headers = match command.0 {
                    MultipleHeaderQuery::GetVerified { from, amount } => {
                        let from_header = from_value(from).ok().unwrap();
                        self.node
                            .request_verified_headers(&from_header, amount)
                            .await
                            .unwrap()
                    }
                    MultipleHeaderQuery::Range {
                        start_height,
                        end_height,
                    } => match (start_height, end_height) {
                        (None, None) => self.node.get_headers(..).await,
                        (Some(start), None) => self.node.get_headers(start..).await,
                        (None, Some(end)) => self.node.get_headers(..=end).await,
                        (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
                    }
                    .ok()
                    .unwrap(),
                };

                let jsvalue = to_value(&headers).ok().unwrap().into(); // TODO: array fix?
                response.send(jsvalue).expect("channel_dropped");
            }
            NodeCommandWithChannel::GetSamplingMetadata((command, response)) => {
                let metadata = self
                    .node
                    .get_sampling_metadata(command.height)
                    .await
                    .ok()
                    .unwrap()
                    .unwrap();
                response.send(metadata).expect("channel_dropped");
            }
        };
    }
}

//type WorkerChannel = BChannel<NodeCommand, NodeResponse>;

enum WorkerMessage {
    NewConnection(MessagePort),
    Command(NodeCommandWithChannel),
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
                    let data: NodeCommand = from_value(message_data).expect("could not from value");

                    let (tx, rx) = oneshot::channel();
                    let command_with_channel = todo!();

                    local_tx
                        .send(WorkerMessage::Command(command_with_channel))
                        .await
                        .expect("send3 err");

                    let response = rx.await.expect("forwardding channel error");
                    let v = to_value(&response).expect("could not to_value");

                    self.ports[client_id]
                        .post_message(&v)
                        .expect("error posting");
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
            WorkerMessage::Command(command_with_channel) => {
                let Some(worker) = &mut worker else {
                    match command_with_channel {
                        NodeCommandWithChannel::IsRunning((_, response)) => {
                            response.send(false).expect("channel_dropped");
                        }
                        NodeCommandWithChannel::StartNode((command, response)) => {
                            worker = Some(NodeWorker::new(command.0).await);
                            response
                                .send(NodeState::NodeStarted)
                                .expect("channel_dropped");
                        }
                        _ => warn!("Worker not running"),
                    }
                    continue;
                };

                trace!("received: {command_with_channel:?}");
                let response = worker.process_command(command_with_channel).await;
                //connector.respond_to(client, response);
            }
        }
    }

    warn!("EXIT EXIT EXIT");
}
