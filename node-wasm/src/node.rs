use std::result::Result as StdResult;

use celestia_node::network::{canonical_network_bootnodes, network_genesis, network_id};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::store::{IndexedDbStore, Store};
use celestia_types::{hash::Hash, ExtendedHeader};
use js_sys::Array;
use libp2p::identity::Keypair;
use libp2p::{identity, Multiaddr};
use serde_wasm_bindgen::{from_value, to_value};
use tracing::info;
use wasm_bindgen::prelude::*;

use crate::utils::js_value_from_display;
use crate::utils::JsContext;
use crate::utils::Network;
use crate::wrapper::libp2p::NetworkInfo;
use crate::Result;

#[wasm_bindgen(js_name = Node)]
struct WasmNode {
    node: Node<IndexedDbStore>,
}

#[wasm_bindgen(js_name = NodeConfig)]
pub struct WasmNodeConfig {
    pub network: Network,
    #[wasm_bindgen(skip)]
    pub genesis_hash: Option<Hash>,
    #[wasm_bindgen(skip)]
    pub p2p_local_keypair: Keypair,
    #[wasm_bindgen(skip)]
    pub p2p_bootnodes: Vec<Multiaddr>,
}

#[wasm_bindgen(js_class = Node)]
impl WasmNode {
    #[wasm_bindgen(constructor)]
    pub async fn new(config: WasmNodeConfig) -> Result<WasmNode> {
        let network_id = network_id(config.network.into());
        let store = IndexedDbStore::new(network_id)
            .await
            .js_context("Failed to open the store")?;

        if let Ok(store_height) = store.head_height().await {
            info!("Initialised store with head height: {store_height}");
        } else {
            info!("Initialized new empty store");
        }

        let node = Node::new(NodeConfig {
            network_id: network_id.to_string(),
            genesis_hash: config.genesis_hash,
            p2p_local_keypair: config.p2p_local_keypair,
            p2p_bootnodes: config.p2p_bootnodes,
            p2p_listen_on: vec![],
            store,
        })
        .await
        .js_context("Failed to start the node")?;

        Ok(Self { node })
    }

    pub fn local_peer_id(&self) -> String {
        self.node.p2p().local_peer_id().to_string()
    }

    pub async fn wait_connected(&self) -> Result<()> {
        Ok(self.node.p2p().wait_connected().await?)
    }

    pub async fn wait_connected_trusted(&self) -> Result<()> {
        Ok(self.node.p2p().wait_connected_trusted().await?)
    }

    pub async fn network_info(&self) -> Result<NetworkInfo> {
        Ok(self.node.p2p().network_info().await?.into())
    }

    pub async fn get_head_header(&self) -> Result<JsValue> {
        let eh = self.node.p2p().get_head_header().await?;
        Ok(to_value(&eh)?)
    }

    pub async fn get_header(&self, hash: &str) -> Result<JsValue> {
        let hash: Hash = hash.parse()?;
        let eh = self.node.p2p().get_header(hash).await?;
        Ok(to_value(&eh)?)
    }

    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let eh = self.node.p2p().get_header_by_height(height).await?;
        Ok(to_value(&eh)?)
    }

    pub async fn get_verified_headers_range(&self, from: JsValue, amount: u64) -> Result<Array> {
        let header =
            from_value::<ExtendedHeader>(from).js_context("Parsing extended header failed")?;
        let verified_headers = self
            .node
            .p2p()
            .get_verified_headers_range(&header, amount)
            .await?;

        Ok(verified_headers
            .iter()
            .map(to_value)
            .collect::<StdResult<_, _>>()?)
    }

    pub async fn listeners(&self) -> Result<Array> {
        let listeners = self.node.p2p().listeners().await?;

        Ok(listeners
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    pub async fn connected_peers(&self) -> Result<Array> {
        Ok(self
            .node
            .p2p()
            .connected_peers()
            .await?
            .iter()
            .map(js_value_from_display)
            .collect::<Array>())
    }

    pub async fn syncer_info(&self) -> Result<JsValue> {
        let syncer_info = self.node.syncer().info().await?;
        Ok(to_value(&syncer_info)?)
    }
}

#[wasm_bindgen(js_class = NodeConfig)]
impl WasmNodeConfig {
    #[wasm_bindgen(constructor)]
    pub fn new(network: Network) -> Self {
        let genesis_hash = network_genesis(network.into());

        let p2p_local_keypair = identity::Keypair::generate_ed25519();
        let p2p_bootnodes = canonical_network_bootnodes(network.into());

        WasmNodeConfig {
            network,
            genesis_hash,
            p2p_local_keypair,
            p2p_bootnodes,
        }
    }

    #[wasm_bindgen(getter)]
    pub fn genesis_hash(&self) -> Option<String> {
        self.genesis_hash.map(|h| h.to_string())
    }

    #[wasm_bindgen(setter)]
    pub fn set_genesis_hash(&mut self, hash: Option<String>) -> Result<()> {
        self.genesis_hash = hash.map(|h| h.parse()).transpose()?;
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn bootnodes(&self) -> Array {
        self.p2p_bootnodes
            .iter()
            .map(js_value_from_display)
            .collect::<Array>()
    }

    #[wasm_bindgen(setter)]
    pub fn set_bootnodes(&mut self, bootnodes: Array) -> Result<()> {
        self.p2p_bootnodes = bootnodes
            .iter()
            .map(|n| {
                n.as_string()
                    .ok_or(JsError::new("utf16 decode error"))
                    .and_then(|s| Ok(s.parse::<Multiaddr>()?))
            })
            .collect::<Result<_>>()?;

        Ok(())
    }
}
