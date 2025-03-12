#![allow(dead_code)]

use std::sync::OnceLock;
use std::time::Duration;

use celestia_rpc::{prelude::*, Client, TxConfig};
use celestia_types::Blob;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use lumina_node::blockstore::InMemoryBlockstore;
use lumina_node::events::EventSubscriber;
use lumina_node::node::Node;
use lumina_node::store::InMemoryStore;
use lumina_node::test_utils::test_node_builder;
use tokio::sync::Mutex;
use tokio::time::sleep;

const WS_URL: &str = "ws://localhost:26658";

pub async fn bridge_client() -> Client {
    Client::new(WS_URL, None).await.unwrap()
}

pub async fn fetch_bridge_info() -> (PeerId, Multiaddr) {
    let client = bridge_client().await;
    let bridge_info = client.p2p_info().await.unwrap();

    let mut ma = bridge_info
        .addrs
        .into_iter()
        .find(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .expect("Bridge doesn't listen on tcp");

    if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
        ma.push(Protocol::P2p(bridge_info.id.into()))
    }

    (bridge_info.id.into(), ma)
}

pub async fn new_connected_node() -> (Node<InMemoryBlockstore, InMemoryStore>, EventSubscriber) {
    let (_, bridge_ma) = fetch_bridge_info().await;

    let (node, events) = test_node_builder()
        .bootnodes([bridge_ma])
        .start_subscribed()
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

    (node, events)
}

pub async fn blob_submit(client: &Client, blobs: &[Blob]) -> u64 {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().await;
    client
        .blob_submit(blobs, TxConfig::default())
        .await
        .unwrap()
}
