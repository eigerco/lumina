#![allow(dead_code)]

use std::env;
use std::time::Duration;

use celestia_rpc::{prelude::*, Client};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use lumina_node::blockstore::InMemoryBlockstore;
use lumina_node::node::NodeConfig;
use lumina_node::test_utils::test_node_config;
use lumina_node::{node::Node, store::InMemoryStore};
use tokio::time::sleep;

const WS_URL: &str = "ws://localhost:26658";

pub async fn fetch_bridge_info() -> (PeerId, Multiaddr) {
    let _ = dotenvy::dotenv();

    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN").unwrap();
    let client = Client::new(WS_URL, Some(&auth_token)).await.unwrap();
    let bridge_info = client.p2p_info().await.unwrap();

    let mut ma = bridge_info
        .addrs
        .into_iter()
        .find(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .expect("Bridge doesn't listen on tcp");

    if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
        ma.push(Protocol::P2p(bridge_info.id.into()))
    }

    println!("PEER: {:?}", bridge_info.id);
    println!("MA: {ma}");

    (bridge_info.id.into(), ma)
}

pub async fn new_connected_node() -> Node<InMemoryBlockstore, InMemoryStore> {
    let (_, bridge_ma) = fetch_bridge_info().await;

    let node = Node::new(NodeConfig {
        p2p_bootnodes: vec![bridge_ma],
        ..test_node_config()
    })
    .await
    .unwrap();

    node.wait_connected_trusted().await.unwrap();

    // Wait until node reaches height 3
    loop {
        if let Some(head) = node.get_network_head_header().await.unwrap() {
            if head.height().value() >= 3 {
                break;
            }
        };

        sleep(Duration::from_secs(1)).await;
    }

    node
}
