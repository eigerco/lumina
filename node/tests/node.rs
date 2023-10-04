use std::env;
use std::time::Duration;

use celestia_node::{
    node::{Node, NodeConfig},
    p2p::{ExchangeError, P2pError, P2pService},
    store::{InMemoryStore, Store},
    test_utils::gen_filled_store,
};
use celestia_rpc::prelude::*;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade::Version},
    identity::{self, Keypair},
    multiaddr::Protocol,
    noise, tcp, yamux, Multiaddr, PeerId, Transport,
};
use tokio::time::sleep;

const WS_URL: &str = "ws://localhost:26658";

fn tcp_transport(local_keypair: &Keypair) -> Boxed<(PeerId, StreamMuxerBox)> {
    tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(local_keypair).unwrap())
        .multiplex(yamux::Config::default())
        .boxed()
}

async fn fetch_bridge_info() -> (PeerId, Multiaddr) {
    let _ = dotenvy::dotenv();

    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN").unwrap();
    let client = celestia_rpc::client::new_websocket(WS_URL, Some(&auth_token))
        .await
        .unwrap();
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
        p2p_bootstrap_peers: vec![bridge_ma],
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
        p2p_bootstrap_peers: vec![bridge_ma],
        p2p_listen_on: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        ..with_keypair(node1_keypair)
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
        p2p_bootstrap_peers: node1_addrs.clone(),
        p2p_listen_on: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        ..with_keypair(node2_keypair)
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
        p2p_bootstrap_peers: node1_addrs.clone(),
        ..with_keypair(node3_keypair)
    })
    .await
    .unwrap();

    node3.p2p().wait_connected().await.unwrap();

    // Small wait until all nodes are discovered and connected
    sleep(Duration::from_millis(500)).await;

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

#[tokio::test]
async fn client_server_test() {
    // Server Node
    let (server_store, mut header_generator) = gen_filled_store(0);
    let server_headers = header_generator.next_many(20);
    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listen_localhost()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.p2p().listeners().await.unwrap();

    // Client node
    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs.clone(),
        ..test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    // request head (with one peer)
    let received_head = client.p2p().get_head_header().await.unwrap();
    assert_eq!(server_headers.last().unwrap(), &received_head);

    // request by hash
    let received_header_by_height = client.p2p().get_header_by_height(10).await.unwrap();
    assert_eq!(server_headers[9], received_header_by_height);

    // request genesis by height
    let received_genesis = client.p2p().get_header_by_height(1).await.unwrap();
    assert_eq!(server_headers.first().unwrap(), &received_genesis);

    // request entire store range
    let received_all_headers = client
        .p2p()
        .get_verified_headers_range(&received_genesis, 19)
        .await
        .unwrap();
    assert_eq!(server_headers[1..], received_all_headers);

    // reqest more headers than available in store
    let out_of_bounds = client
        .p2p()
        .get_verified_headers_range(&received_genesis, 20)
        .await
        .unwrap_err();
    assert!(matches!(
        out_of_bounds,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    // request unknown hash
    let unstored_header = header_generator.next_of(&server_headers[0]);
    let unexpected_hash = client
        .p2p()
        .get_header(unstored_header.hash())
        .await
        .unwrap_err();
    assert!(matches!(
        unexpected_hash,
        P2pError::Exchange(ExchangeError::HeaderNotFound)
    ));

    // request unknown height
    let unexpected_height = client.p2p().get_header_by_height(21).await.unwrap_err();
    assert!(matches!(
        unexpected_height,
        P2pError::Exchange(ExchangeError::HeaderNotFound)
    ));
}

#[tokio::test]
async fn multiple_peers_test() {
    let (server_store, mut header_generator) = gen_filled_store(0);
    let common_server_headers = header_generator.next_many(20);
    server_store
        .append_unchecked(common_server_headers.clone())
        .await
        .unwrap();

    // Server group A, nodes with synced stores
    let mut servers = vec![
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listen_localhost()
        })
        .await
        .unwrap(),
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listen_localhost()
        })
        .await
        .unwrap(),
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listen_localhost()
        })
        .await
        .unwrap(),
    ];

    // Server group B, single node with additional headers
    let additional_server_headers = header_generator.next_many(5);
    server_store
        .append_unchecked(additional_server_headers)
        .await
        .unwrap();

    servers.push(
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listen_localhost()
        })
        .await
        .unwrap(),
    );

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;

    let mut server_addrs = vec![];
    for s in servers {
        server_addrs.extend_from_slice(&s.p2p().listeners().await.unwrap()[..]);
    }

    // Client Node
    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    let network_head = client.p2p().get_head_header().await.unwrap();
    assert_eq!(common_server_headers.last().unwrap(), &network_head);
}

#[tokio::test]
async fn test_replaced_header() {
    // Server node, header at height 11 shouldn't pass verification as it's been replaced
    let (server_store, mut header_generator) = gen_filled_store(0);
    let mut server_headers = header_generator.next_many(20);
    let replaced_header = header_generator.another_of(&server_headers[10]);
    server_headers[10] = replaced_header.clone();

    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listen_localhost()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.p2p().listeners().await.unwrap();

    let node2 = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..listen_localhost()
    })
    .await
    .unwrap();

    node2.p2p().wait_connected().await.unwrap();

    let replaced_header_in_range = node2
        .p2p()
        .get_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        replaced_header_in_range,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    let requested_replaced_header = node2
        .p2p()
        .get_verified_headers_range(&replaced_header, 1)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_replaced_header,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));
}

// helpers to use with struct update syntax to avoid spelling out all the details
fn test_node_config() -> NodeConfig<InMemoryStore> {
    let node_keypair = identity::Keypair::generate_ed25519();
    NodeConfig {
        network_id: "private".to_string(),
        p2p_transport: tcp_transport(&node_keypair),
        p2p_local_keypair: node_keypair,
        p2p_bootstrap_peers: vec![],
        p2p_listen_on: vec![],
        store: InMemoryStore::new(),
    }
}

fn listen_localhost() -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_listen_on: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        ..test_node_config()
    }
}

fn with_keypair(keypair: Keypair) -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_transport: tcp_transport(&keypair),
        p2p_local_keypair: keypair,
        ..test_node_config()
    }
}
