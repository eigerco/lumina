use std::result::Result as StdResult;

use celestia_node::network::{canonical_network_bootnodes, network_genesis, network_id};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::store::{IndexedDbStore, Store};
use celestia_types::{hash::Hash, ExtendedHeader};
use js_sys::Array;
use libp2p::{identity::Keypair, Multiaddr};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tracing::info;
use wasm_bindgen::prelude::*;

use crate::utils::js_value_from_display;
use crate::utils::JsContext;
use crate::utils::Network;
use crate::wrapper::libp2p::NetworkInfo;
use crate::Result;

#[wasm_bindgen(js_name = Node)]
struct WasmNode(Node<IndexedDbStore>);

#[derive(Serialize, Deserialize)]
pub struct WasmNodeConfig {
    pub network: Network,
    pub genesis_hash: Option<Hash>,
    pub bootnodes: Vec<Multiaddr>,
}

#[wasm_bindgen]
pub fn default_config(network: Network) -> Result<JsValue> {
    Ok(to_value(&WasmNodeConfig {
        network,
        genesis_hash: network_genesis(network.into()),
        bootnodes: canonical_network_bootnodes(network.into()),
    })?)
}

#[wasm_bindgen(js_class = Node)]
impl WasmNode {
    #[wasm_bindgen(constructor)]
    pub async fn new(config: JsValue) -> Result<WasmNode> {
        let config = from_value::<WasmNodeConfig>(config)?;

        let network_id = network_id(config.network.into());
        let store = IndexedDbStore::new(network_id)
            .await
            .js_context("Failed to open the store")?;

        if let Ok(store_height) = store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialized new empty store");
        }

        let p2p_local_keypair = Keypair::generate_ed25519();

        let node = Node::new(NodeConfig {
            network_id: network_id.to_string(),
            genesis_hash: config.genesis_hash,
            p2p_local_keypair,
            p2p_bootnodes: config.bootnodes,
            p2p_listen_on: vec![],
            store,
        })
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
    pub async fn network_info(&self) -> Result<NetworkInfo> {
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
        let maybe_head_hedaer = self.0.get_network_head_header();
        Ok(to_value(&maybe_head_hedaer)?)
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
}
