//! A browser compatible wrappers for the [`lumina-node`].

use std::result::Result as StdResult;

use js_sys::Array;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use lumina_node::blockstore::IndexedDbBlockstore;
use lumina_node::network::{canonical_network_bootnodes, network_genesis, network_id};
use lumina_node::node::{Node, NodeConfig};
use lumina_node::store::IndexedDbStore;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tracing::info;
use wasm_bindgen::prelude::*;
use web_sys::{SharedWorker, WorkerOptions, WorkerType};

use crate::utils::{js_value_from_display, BChannel, JsContext, Network};
use crate::worker::{
    GetConnectedPeers, GetHeader, GetListeners, GetLocalPeerId, GetMultipleHeaders, GetNetworkInfo,
    GetPeerTrackerInfo, GetSamplingMetadata, GetSyncerInfo, IsRunning, MultipleHeaderQuery,
    NodeCommand, NodeResponse, RequestHeader, RequestMultipleHeaders, SetPeerTrust,
    SingleHeaderQuery, StartNode, WaitConnected,
};
use crate::wrapper::libp2p::NetworkInfoSnapshot;
use crate::Result;

/// Config for the lumina wasm node.
#[wasm_bindgen(js_name = NodeConfig)]
#[derive(Serialize, Deserialize, Debug)]
pub struct WasmNodeConfig {
    /// A network to connect to.
    pub network: Network,
    /// Hash of the genesis block in the network.
    #[wasm_bindgen(getter_with_clone)]
    pub genesis_hash: Option<String>,
    /// A list of bootstrap peers to connect to.
    #[wasm_bindgen(getter_with_clone)]
    pub bootnodes: Vec<String>,
}

// TODO: add on_error handler
#[wasm_bindgen]
struct NodeDriver {
    _worker: SharedWorker,
    channel: BChannel<NodeCommand, NodeResponse>,
}

#[wasm_bindgen]
impl NodeDriver {
    #[wasm_bindgen(constructor)]
    pub async fn new() -> NodeDriver {
        let mut opts = WorkerOptions::new();
        opts.type_(WorkerType::Module);
        opts.name("lumina");
        let worker = SharedWorker::new_with_worker_options("/js/worker.js", &opts)
            .expect("could not worker");

        let channel = BChannel::new(worker.port());

        Self {
            _worker: worker,
            channel,
        }
    }

    pub async fn is_running(&self) -> bool {
        let response = self.channel.send(IsRunning);

        response.await.unwrap()
    }

    pub async fn start(&self, config: WasmNodeConfig) -> Result<()> {
        let command = StartNode(config);
        let response = self.channel.send(command);

        Ok(response.await.unwrap()?)
    }

    pub async fn local_peer_id(&self) -> String {
        let response = self.channel.send(GetLocalPeerId);

        response.await.unwrap()
    }

    pub async fn peer_tracker_info(&self) -> Result<JsValue> {
        let response = self.channel.send(GetPeerTrackerInfo);

        Ok(to_value(&response.await.unwrap())?)
    }

    pub async fn wait_connected(&self) {
        let command = WaitConnected { trusted: false };
        let response = self.channel.send(command);

        response.await.unwrap()
    }
    pub async fn wait_connected_trusted(&self) {
        let command = WaitConnected { trusted: false };
        let response = self.channel.send(command);

        response.await.unwrap()
    }

    pub async fn network_info(&self) -> Result<NetworkInfoSnapshot> {
        let response = self.channel.send(GetNetworkInfo);

        Ok(response.await.unwrap())
    }

    pub async fn listeners(&self) -> Result<Array> {
        let response = self.channel.send(GetListeners);
        let response = response
            .await
            .unwrap()
            .iter()
            .map(js_value_from_display)
            .collect();

        Ok(response)
    }

    pub async fn connected_peers(&self) -> Result<Array> {
        let response = self.channel.send(GetConnectedPeers);
        let response = response
            .await
            .unwrap()
            .iter()
            .map(js_value_from_display)
            .collect();

        Ok(response)
    }

    pub async fn set_peer_trust(&self, peer_id: &str, is_trusted: bool) -> Result<()> {
        let command = SetPeerTrust {
            peer_id: peer_id.to_string(),
            is_trusted,
        };
        let response = self.channel.send(command);

        Ok(response.await.unwrap())
    }

    pub async fn request_head_header(&self) -> Result<JsValue> {
        let command = RequestHeader(SingleHeaderQuery::Head);
        let response = self.channel.send(command);
        Ok(to_value(&response.await.unwrap()?)?)
    }

    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = RequestHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap()?)?)
    }

    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = RequestHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap()?)?)
    }

    pub async fn request_verified_headers(
        &self,
        from_header: JsValue,
        amount: u64,
    ) -> Result<Array> {
        let from = from_value(from_header)?;
        let command = RequestMultipleHeaders(MultipleHeaderQuery::GetVerified { from, amount });
        let response = self.channel.send(command);

        let result = response
            .await
            .iter()
            .map(|h| to_value(&h).unwrap()) // XXX
            .collect();

        Ok(result)
    }

    pub async fn syncer_info(&self) -> Result<JsValue> {
        let response = self.channel.send(GetSyncerInfo);

        Ok(to_value(&response.await.unwrap())?)
    }

    pub async fn get_network_head_header(&self) -> Result<JsValue> {
        todo!()
        /*
                let command = todo!();
                let response = self.channel.send(command);

                Ok(to_value(&response.await.unwrap())?)
        */
    }

    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let command = GetHeader(SingleHeaderQuery::Head);
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap())?)
    }

    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = GetHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap())?)
    }
    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = GetHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap())?)
    }
    pub async fn get_headers(
        &self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Array> {
        let command = GetMultipleHeaders(MultipleHeaderQuery::Range {
            start_height,
            end_height,
        });
        let response = self.channel.send(command);
        let result = response
            .await
            .iter()
            .map(|h| to_value(&h).unwrap())
            .collect();

        Ok(result)
    }
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        let command = GetSamplingMetadata { height };
        let response = self.channel.send(command);

        Ok(to_value(&response.await.unwrap())?)
    }
}

/*
#[wasm_bindgen(js_class = Node)]
impl WasmNode {
    /// Create a new Lumina node.
    #[wasm_bindgen(constructor)]
    pub async fn new(config: WasmNodeConfig) -> Result<WasmNode> {
        let config = config.into_node_config().await?;

        if let Ok(store_height) = config.store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialized new empty store");
        }

        let node = Node::new(config)
            .await
            .js_context("Failed to start the node")?;

        Ok(Self(node))
    }

    /// Get node's local peer ID.
    pub fn local_peer_id(&self) -> String {
        self.0.local_peer_id().to_string()
    }

    /// Get current [`PeerTracker`] info.
    pub fn peer_tracker_info(&self) -> Result<JsValue> {
        let peer_tracker_info = self.0.peer_tracker_info();
        Ok(to_value(&peer_tracker_info)?)
    }

    /// Wait until the node is connected to at least 1 peer.
    pub async fn wait_connected(&self) -> Result<()> {
        Ok(self.0.wait_connected().await?)
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        Ok(self.0.wait_connected_trusted().await?)
    }

    /// Get current network info.
    pub async fn network_info(&self) -> Result<NetworkInfoSnapshot> {
        Ok(self.0.network_info().await?.into())
    }

    /// Get all the multiaddresses on which the node listens.
    pub async fn listeners(&self) -> Result<Array> {
        let listeners = self.0.listeners().await?;

        Ok(listeners
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    /// Get all the peers that node is connected to.
    pub async fn connected_peers(&self) -> Result<Array> {
        Ok(self
            .0
            .connected_peers()
            .await?
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    /// Trust or untrust the peer with a given ID.
    pub async fn set_peer_trust(&self, peer_id: &str, is_trusted: bool) -> Result<()> {
        let peer_id = peer_id.parse().js_context("Parsing peer id failed")?;
        Ok(self.0.set_peer_trust(peer_id, is_trusted).await?)
    }

    /// Request the head header from the network.
    pub async fn request_head_header(&self) -> Result<JsValue> {
        let eh = self.0.request_head_header().await?;
        Ok(to_value(&eh)?)
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let hash: Hash = hash.parse()?;
        let eh = self.0.request_header_by_hash(&hash).await?;
        Ok(to_value(&eh)?)
    }

    /// Request a header for the block with a given height from the network.
    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let eh = self.0.request_header_by_height(height).await?;
        Ok(to_value(&eh)?)
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    pub async fn request_verified_headers(&self, from: JsValue, amount: u64) -> Result<Array> {
        let header =
            from_value::<ExtendedHeader>(from).js_context("Parsing extended header failed")?;
        let verified_headers = self.0.request_verified_headers(&header, amount).await?;

        Ok(verified_headers
            .iter()
            .map(to_value)
            .collect::<StdResult<_, _>>()?)
    }

    /// Get current header syncing info.
    pub async fn syncer_info(&self) -> Result<JsValue> {
        let syncer_info = self.0.syncer_info().await?;
        Ok(to_value(&syncer_info)?)
    }

    /// Get the latest header announced in the network.
    pub fn get_network_head_header(&self) -> Result<JsValue> {
        let maybe_head_header = self.0.get_network_head_header();
        Ok(to_value(&maybe_head_header)?)
    }

    /// Get the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let local_head = self.0.get_local_head_header().await?;
        Ok(to_value(&local_head)?)
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let hash: Hash = hash.parse().js_context("parsing hash failed")?;
        let eh = self.0.get_header_by_hash(&hash).await?;
        Ok(to_value(&eh)?)
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let eh = self.0.get_header_by_height(height).await?;
        Ok(to_value(&eh)?)
    }

    /// Get synced headers from the given heights range.
    ///
    /// If start of the range is undefined (None), the first returned header will be of height 1.
    /// If end of the range is undefined (None), the last returned header will be the last header in the
    /// store.
    ///
    /// # Errors
    ///
    /// If range contains a height of a header that is not found in the store.
    pub async fn get_headers(
        &self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<JsValue> {
        let headers = match (start_height, end_height) {
            (None, None) => self.0.get_headers(..).await,
            (Some(start), None) => self.0.get_headers(start..).await,
            (None, Some(end)) => self.0.get_headers(..=end).await,
            (Some(start), Some(end)) => self.0.get_headers(start..=end).await,
        }?;

        Ok(to_value(&headers)?)
    }

    /// Get data sampling metadata of an already sampled height.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        let metadata = self.0.get_sampling_metadata(height).await?;

        #[derive(Serialize)]
        struct Intermediate {
            accepted: bool,
            cids_sampled: Vec<String>,
        }

        let metadata = metadata.map(|m| Intermediate {
            accepted: m.accepted,
            cids_sampled: m
                .cids_sampled
                .into_iter()
                .map(|cid| cid.to_string())
                .collect(),
        });

        Ok(to_value(&metadata)?)
    }
}
*/

#[wasm_bindgen(js_class = NodeConfig)]
impl WasmNodeConfig {
    /// Get the configuration with default bootnodes and genesis hash for provided network
    pub fn default(network: Network) -> WasmNodeConfig {
        WasmNodeConfig {
            network,
            genesis_hash: network_genesis(network.into()).map(|h| h.to_string()),
            bootnodes: canonical_network_bootnodes(network.into())
                .filter(|addr| addr.iter().any(|proto| proto == Protocol::WebTransport))
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>(),
        }
    }

    pub(crate) async fn into_node_config(
        self,
    ) -> Result<NodeConfig<IndexedDbBlockstore, IndexedDbStore>> {
        let network_id = network_id(self.network.into());
        let store = IndexedDbStore::new(network_id)
            .await
            .js_context("Failed to open the store")?;
        let blockstore = IndexedDbBlockstore::new(&format!("{network_id}-blockstore"))
            .await
            .js_context("Failed to open the blockstore")?;

        let p2p_local_keypair = Keypair::generate_ed25519();

        let genesis_hash = self.genesis_hash.map(|h| h.parse()).transpose()?;
        let p2p_bootnodes = self
            .bootnodes
            .iter()
            .map(|addr| addr.parse())
            .collect::<StdResult<_, _>>()?;

        Ok(NodeConfig {
            network_id: network_id.to_string(),
            genesis_hash,
            p2p_bootnodes,
            p2p_local_keypair,
            p2p_listen_on: vec![],
            blockstore,
            store,
        })
    }
}
