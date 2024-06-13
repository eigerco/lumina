//! A browser compatible wrappers for the [`lumina-node`].

use std::result::Result as StdResult;

use celestia_types::{hash::Hash, ExtendedHeader};
use js_sys::Array;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use lumina_node::blockstore::IndexedDbBlockstore;
use lumina_node::events::{EventSubscriber, NodeEventInfo};
use lumina_node::network::{canonical_network_bootnodes, network_genesis, network_id};
use lumina_node::node::{Node, NodeConfig};
use lumina_node::store::{IndexedDbStore, SamplingStatus, Store};
use serde::Serialize;
use serde_wasm_bindgen::{from_value, to_value};
use tracing::info;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::BroadcastChannel;

use crate::error::{Context, Result};
use crate::utils::{get_crypto, js_value_from_display, Network};
use crate::wrapper::libp2p::NetworkInfo;

/// Lumina wasm node.
#[wasm_bindgen(js_name = Node)]
pub struct WasmNode {
    node: Node<IndexedDbStore>,
    events_channel_name: String,
}

/// Config for the lumina wasm node.
#[wasm_bindgen(js_name = NodeConfig)]
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
            .context("Failed to start the node")?;

        let events_channel_name = format!("NodeEventChannel-{}", get_crypto()?.random_uuid());
        let events_channel = BroadcastChannel::new(&events_channel_name)
            .context("Failed to allocate BroadcastChannel")?;

        let events_sub = node.event_subscriber();
        spawn_local(event_forwarder_task(events_sub, events_channel));

        Ok(WasmNode {
            node,
            events_channel_name,
        })
    }

    /// Get node's local peer ID.
    pub fn local_peer_id(&self) -> String {
        self.node.local_peer_id().to_string()
    }

    /// Get current `PeerTracker` info.
    pub fn peer_tracker_info(&self) -> Result<JsValue> {
        let peer_tracker_info = self.node.peer_tracker_info();
        Ok(to_value(&peer_tracker_info)?)
    }

    /// Wait until the node is connected to at least 1 peer.
    pub async fn wait_connected(&self) -> Result<()> {
        Ok(self.node.wait_connected().await?)
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        Ok(self.node.wait_connected_trusted().await?)
    }

    /// Get current network info.
    pub async fn network_info(&self) -> Result<NetworkInfo> {
        Ok(self.node.network_info().await?.into())
    }

    /// Get all the multiaddresses on which the node listens.
    pub async fn listeners(&self) -> Result<Array> {
        let listeners = self.node.listeners().await?;

        Ok(listeners
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    /// Get all the peers that node is connected to.
    pub async fn connected_peers(&self) -> Result<Array> {
        Ok(self
            .node
            .connected_peers()
            .await?
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    /// Trust or untrust the peer with a given ID.
    pub async fn set_peer_trust(&self, peer_id: &str, is_trusted: bool) -> Result<()> {
        let peer_id = peer_id.parse().context("Parsing peer id failed")?;
        Ok(self.node.set_peer_trust(peer_id, is_trusted).await?)
    }

    /// Request the head header from the network.
    pub async fn request_head_header(&self) -> Result<JsValue> {
        let eh = self.node.request_head_header().await?;
        Ok(to_value(&eh)?)
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let hash: Hash = hash.parse()?;
        let eh = self.node.request_header_by_hash(&hash).await?;
        Ok(to_value(&eh)?)
    }

    /// Request a header for the block with a given height from the network.
    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let eh = self.node.request_header_by_height(height).await?;
        Ok(to_value(&eh)?)
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    pub async fn request_verified_headers(&self, from: JsValue, amount: u64) -> Result<Array> {
        let header =
            from_value::<ExtendedHeader>(from).context("Parsing extended header failed")?;
        let verified_headers = self.node.request_verified_headers(&header, amount).await?;

        Ok(verified_headers
            .iter()
            .map(to_value)
            .collect::<StdResult<_, _>>()?)
    }

    /// Get current header syncing info.
    pub async fn syncer_info(&self) -> Result<JsValue> {
        let syncer_info = self.node.syncer_info().await?;
        Ok(to_value(&syncer_info)?)
    }

    /// Get the latest header announced in the network.
    pub fn get_network_head_header(&self) -> Result<JsValue> {
        let maybe_head_hedaer = self.node.get_network_head_header();
        Ok(to_value(&maybe_head_hedaer)?)
    }

    /// Get the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let local_head = self.node.get_local_head_header().await?;
        Ok(to_value(&local_head)?)
    }

    /// Get ranges of headers currently stored.
    pub async fn get_stored_header_ranges(&self) -> Result<Array> {
        let ranges = self.node.get_stored_header_ranges().await?;

        Ok(ranges
            .as_ref()
            .iter()
            .map(to_value)
            .collect::<Result<_, _>>()?)
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let hash: Hash = hash.parse().context("parsing hash failed")?;
        let eh = self.node.get_header_by_hash(&hash).await?;
        Ok(to_value(&eh)?)
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let eh = self.node.get_header_by_height(height).await?;
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
            (None, None) => self.node.get_headers(..).await,
            (Some(start), None) => self.node.get_headers(start..).await,
            (None, Some(end)) => self.node.get_headers(..=end).await,
            (Some(start), Some(end)) => self.node.get_headers(start..=end).await,
        }?;

        Ok(to_value(&headers)?)
    }

    /// Get data sampling metadata of an already sampled height.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        let metadata = self.node.get_sampling_metadata(height).await?;

        #[derive(Serialize)]
        struct Intermediate {
            status: SamplingStatus,
            cids: Vec<String>,
        }

        let metadata = metadata.map(|m| Intermediate {
            status: m.status,
            cids: m.cids.into_iter().map(|cid| cid.to_string()).collect(),
        });

        Ok(to_value(&metadata)?)
    }

    /// Returns a [`BroadcastChannel`] for events generated by [`Node`].
    pub fn events_channel(&self) -> Result<BroadcastChannel> {
        Ok(BroadcastChannel::new(&self.events_channel_name).unwrap())
    }
}

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

    async fn into_node_config(self) -> Result<NodeConfig<IndexedDbBlockstore, IndexedDbStore>> {
        let network_id = network_id(self.network.into());
        let store = IndexedDbStore::new(network_id)
            .await
            .context("Failed to open the store")?;
        let blockstore = IndexedDbBlockstore::new(&format!("{network_id}-blockstore"))
            .await
            .context("Failed to open the blockstore")?;

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

async fn event_forwarder_task(mut events_sub: EventSubscriber, events_channel: BroadcastChannel) {
    #[derive(Serialize)]
    struct Event {
        message: String,
        #[serde(flatten)]
        info: NodeEventInfo,
    }

    while let Ok(ev) = events_sub.recv().await {
        let ev = Event {
            message: ev.event.to_string(),
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
