use anyhow::Context;
use celestia_node::network::{network_id, canonical_network_bootnodes, network_genesis};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::store::IndexedDbStore;
use celestia_types::hash::Hash;
use js_sys::Array;
use libp2p::identity::Keypair;
use libp2p::{identity, Multiaddr};
use serde_wasm_bindgen::to_value;
use tracing::info;
use wasm_bindgen::prelude::*;

use crate::utils::Network;
use crate::wrapper::libp2p::NetworkInfo;

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
    pub async fn new(config: WasmNodeConfig) -> Self {
        let network_id = network_id(config.network.into());
        let store = IndexedDbStore::new(network_id).await.unwrap();
        info!(
            "Initialised store with head height: {:?}",
            store.get_head_height()
        );

        let node = Node::new(NodeConfig {
            network_id: network_id.to_string(),
            genesis_hash: config.genesis_hash,
            p2p_local_keypair: config.p2p_local_keypair,
            p2p_bootnodes: config.p2p_bootnodes,
            p2p_listen_on: vec![],
            store,
        })
        .await
        .context("Failed to start node")
        .unwrap_throw();

        Self { node }
    }

    pub fn local_peer_id(&self) -> String {
        self.node.p2p().local_peer_id().to_string()
    }

    pub async fn wait_connected(&self) {
        self.node.p2p().wait_connected().await.unwrap_throw();
    }

    pub async fn wait_connected_trusted(&self) {
        self.node
            .p2p()
            .wait_connected_trusted()
            .await
            .unwrap_throw();
    }

    pub async fn network_info(&self) -> NetworkInfo {
        self.node.p2p().network_info().await.unwrap_throw().into()
    }

    pub async fn get_head_header(&self) -> JsValue {
        let eh = self.node.p2p().get_head_header().await.unwrap_throw();
        to_value(&eh).unwrap_throw()
    }

    pub async fn get_header(&self, hash: &str /*&Hash*/) -> JsValue {
        let hash = hash.parse().unwrap_throw();
        let eh = self.node.p2p().get_header(hash).await.unwrap_throw();
        to_value(&eh).unwrap_throw()
    }

    pub async fn get_header_by_height(&self, height: u64) -> JsValue {
        let eh = self
            .node
            .p2p()
            .get_header_by_height(height)
            .await
            .unwrap_throw();
        to_value(&eh).unwrap_throw()
    }

    pub async fn get_verified_headers_range() {
        unimplemented!()
    }

    pub async fn listeners(&self) -> Array {
        self.node
            .p2p()
            .listeners()
            .await
            .unwrap_throw()
            .iter()
            .map(ToString::to_string)
            .map(JsValue::from)
            .collect::<Array>()
    }

    pub async fn connected_peers(&self) -> Array {
        self.node
            .p2p()
            .connected_peers()
            .await
            .unwrap_throw()
            .iter()
            .map(ToString::to_string)
            .map(JsValue::from)
            .collect::<Array>()
    }

    pub async fn syncer_info(&self) -> JsValue {
        let syncer_info = self.node.syncer().info().await.unwrap_throw();
        to_value(&syncer_info).unwrap_throw()
    }
}

#[wasm_bindgen(js_class = NodeConfig)]
impl WasmNodeConfig {
    #[wasm_bindgen(constructor)]
    pub fn new(network: Network) -> Self {
        let p2p_local_keypair = identity::Keypair::generate_ed25519();
        let genesis_hash = network_genesis(network.into());

        let mut p2p_bootnodes = canonical_network_bootnodes(network.into());
        if network == Network::Mocha {
            // 40.85.94.176 is a node set up for testing QUIC/WebTransport since official nodes
            // don't have that enabled currently
            let webtransport_bootnode_addrs = [
                "/ip4/40.85.94.176/tcp/2121/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
                "/ip4/40.85.94.176/udp/2121/quic-v1/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
                "/ip4/40.85.94.176/udp/2121/quic-v1/webtransport/certhash/uEiBf-OX4HzFK9owOpjdCifsDIWRO0SoD3j3vGKlq0pAXKw/certhash/uEiCx1md1BATJ_0NXAjp3KOuwRYG1535E7kUzFdMq8aPaWw/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
                "/ip4/40.85.94.176/udp/2121/quic-v1/p2p/12D3KooWQUYAApYb4DJnhS1QmAwRr5HRvUeHJYocchCpwEhCtDGu",
                "/ip4/40.85.94.176/udp/2121/quic-v1/webtransport/certhash/uEiBr4-sr95BpqfA-ttpjiLdjbGABhTvX8oxrTXf3Ubfibw/certhash/uEiBSVgyze9xG1UbbNuTwyEUWLPq7l2N9pyeQSs3OtEhGRg/p2p/12D3KooWQUYAApYb4DJnhS1QmAwRr5HRvUeHJYocchCpwEhCtDGu",
            ].iter().map(|s| s.parse().unwrap_throw());

            p2p_bootnodes.extend(webtransport_bootnode_addrs);
        }

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
    pub fn set_genesis_hash(&mut self, hash: Option<String>) {
        self.genesis_hash = hash.map(|h| h.parse().unwrap_throw())
    }

    #[wasm_bindgen(getter)]
    pub fn bootnodes(&self) -> Array {
        self.p2p_bootnodes
            .iter()
            .map(ToString::to_string)
            .map(JsValue::from)
            .collect::<Array>()
    }

    #[wasm_bindgen(setter)]
    pub fn set_bootnodes(&mut self, bootnodes: Array) {
        self.p2p_bootnodes = bootnodes
            .iter()
            .map(|addr| addr.as_string().unwrap_throw().parse().unwrap_throw())
            .collect::<Vec<_>>();
    }
}
