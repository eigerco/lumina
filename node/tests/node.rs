use std::env;

use celestia_node::{
    node::{Node, NodeConfig},
    p2p::P2pService,
};
use celestia_rpc::prelude::*;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade::Version},
    identity::{self, Keypair},
    noise, tcp, yamux, Multiaddr, PeerId, Transport,
};

const WS_URL: &str = "ws://localhost:26658";

fn tcp_transport(local_keypair: &Keypair) -> Boxed<(PeerId, StreamMuxerBox)> {
    tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(local_keypair).unwrap())
        .multiplex(yamux::Config::default())
        .boxed()
}

async fn get_bridge_tcp_ma() -> Multiaddr {
    let _ = dotenvy::dotenv();

    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN").unwrap();
    let client = celestia_rpc::client::new_websocket(WS_URL, Some(&auth_token))
        .await
        .unwrap();

    let bridge_info = client.p2p_info().await.unwrap();

    bridge_info
        .addrs
        .into_iter()
        .find(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .expect("Bridge doesn't listen on tcp")
}

async fn new_connected_node() -> Node {
    let bridge_ma = get_bridge_tcp_ma().await;
    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let node = Node::new(NodeConfig {
        network_id: "private".to_string(),
        p2p_transport: tcp_transport(&p2p_local_keypair),
        p2p_local_keypair,
        p2p_bootstrap_peers: vec![bridge_ma],
        p2p_listen_on: vec![],
    })
    .await
    .unwrap();

    node.p2p().wait_connected().await.unwrap();

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
    let head = node.p2p().get_head_header().await.unwrap();

    genesis.verify(&head).unwrap();
}
