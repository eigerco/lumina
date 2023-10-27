#![cfg(not(target_arch = "wasm32"))]

use std::env;
use std::time::Duration;

use celestia_node::{
    node::{Node, NodeConfig},
    store::InMemoryStore,
    test_utils::{test_node_config, test_node_config_with_keypair},
};
use celestia_rpc::{prelude::*, Client};
use libp2p::{identity, multiaddr::Protocol, Multiaddr, PeerId};
use tokio::time::sleep;

const WS_URL: &str = "ws://localhost:26658";

async fn fetch_bridge_info() -> (PeerId, Multiaddr) {
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

    (bridge_info.id.into(), ma)
}

async fn new_connected_node() -> Node<InMemoryStore> {
    let (_, bridge_ma) = fetch_bridge_info().await;

    let node = Node::new(NodeConfig {
        p2p_bootnodes: vec![bridge_ma],
        ..test_node_config()
    })
    .await
    .unwrap();

    node.p2p().wait_connected().await.unwrap();

    // Wait until node reaches height 3
    loop {
        let head = node.p2p().get_head_header().await.unwrap();

        if head.height().value() >= 3 {
            break;
        }

        sleep(Duration::from_secs(1)).await;
    }

    node
}

#[tokio::test]
async fn connects_to_the_go_bridge_node() {
    let node = new_connected_node().await;

    let info = node.p2p().network_info().await.unwrap();
    assert_eq!(info.num_peers(), 1);
}

#[tokio::test]
async fn get_single_header() {
    let node = new_connected_node().await;

    let header = node.p2p().get_header_by_height(1).await.unwrap();
    let header_by_hash = node.p2p().get_header(header.hash()).await.unwrap();

    assert_eq!(header, header_by_hash);
}

#[tokio::test]
async fn get_verified_headers() {
    let node = new_connected_node().await;

    let from = node.p2p().get_header_by_height(1).await.unwrap();
    let verified_headers = node
        .p2p()
        .get_verified_headers_range(&from, 2)
        .await
        .unwrap();
    assert_eq!(verified_headers.len(), 2);

    let height2 = node.p2p().get_header_by_height(2).await.unwrap();
    assert_eq!(verified_headers[0], height2);

    let height3 = node.p2p().get_header_by_height(3).await.unwrap();
    assert_eq!(verified_headers[1], height3);
}

#[tokio::test]
async fn get_head() {
    let node = new_connected_node().await;

    let genesis = node.p2p().get_header_by_height(1).await.unwrap();

    let head1 = node.p2p().get_head_header().await.unwrap();
    genesis.verify(&head1).unwrap();

    let head2 = node.p2p().get_header_by_height(0).await.unwrap();
    assert!(head1 == head2 || head1.verify(&head2).is_ok());
}

#[tokio::test]
async fn peer_discovery() {
    // Bridge node cannot connect to other nodes because it is behind Docker's NAT.
    // However Node2 and Node3 can discover its address via Node1.
    let (bridge_peer_id, bridge_ma) = fetch_bridge_info().await;

    // Node1
    //
    // This node connects to Bridge node.
    let node1_keypair = identity::Keypair::generate_ed25519();
    let node1_peer_id = PeerId::from(node1_keypair.public());
    let node1 = Node::new(NodeConfig {
        p2p_bootnodes: vec![bridge_ma],
        p2p_listen_on: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        ..test_node_config_with_keypair(node1_keypair)
    })
    .await
    .unwrap();

    node1.p2p().wait_connected().await.unwrap();

    let node1_addrs = node1.p2p().listeners().await.unwrap();

    // Node2
    //
    // This node connects to Node1 and will discover Bridge node.
    let node2_keypair = identity::Keypair::generate_ed25519();
    let node2_peer_id = PeerId::from(node2_keypair.public());
    let node2 = Node::new(NodeConfig {
        p2p_bootnodes: node1_addrs.clone(),
        p2p_listen_on: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        ..test_node_config_with_keypair(node2_keypair)
    })
    .await
    .unwrap();

    node2.p2p().wait_connected().await.unwrap();

    // Node3
    //
    // This node connects to Node1 and will discover Node2 and Bridge node.
    let node3_keypair = identity::Keypair::generate_ed25519();
    let node3_peer_id = PeerId::from(node3_keypair.public());
    let node3 = Node::new(NodeConfig {
        p2p_bootnodes: node1_addrs.clone(),
        ..test_node_config_with_keypair(node3_keypair)
    })
    .await
    .unwrap();

    node3.p2p().wait_connected().await.unwrap();

    // Small wait until all nodes are discovered and connected
    sleep(Duration::from_millis(800)).await;

    // Check Node1 connected peers
    let connected_peers = node1.p2p().connected_peers().await.unwrap();
    assert!(connected_peers.iter().any(|peer| *peer == bridge_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node2_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node3_peer_id));

    // Check Node2 connected peers
    let connected_peers = node2.p2p().connected_peers().await.unwrap();
    assert!(connected_peers.iter().any(|peer| *peer == bridge_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node1_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node3_peer_id));

    // Check Node3 connected peers
    let connected_peers = node3.p2p().connected_peers().await.unwrap();
    assert!(connected_peers.iter().any(|peer| *peer == bridge_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node1_peer_id));
    assert!(connected_peers.iter().any(|peer| *peer == node2_peer_id));
}
