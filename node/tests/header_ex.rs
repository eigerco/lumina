#![cfg(not(tarrequest_arch = "wasm32"))]

use std::time::Duration;

use celestia_node::{
    node::{Node, NodeConfig, NodeError},
    p2p::{HeaderExError, P2pError},
    store::Store,
    test_utils::{gen_filled_store, listening_test_node_config, test_node_config},
};
use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::test_utils::{invalidate, unverify};
use tokio::time::sleep;

use crate::utils::new_connected_node;

mod utils;

#[tokio::test]
async fn request_single_header() {
    let node = new_connected_node().await;

    let header = node.request_header_by_height(1).await.unwrap();
    let header_by_hash = node.request_header_by_hash(&header.hash()).await.unwrap();

    assert_eq!(header, header_by_hash);
}

#[tokio::test]
async fn request_verified_headers() {
    let node = new_connected_node().await;

    let from = node.request_header_by_height(1).await.unwrap();
    let verified_headers = node.request_verified_headers_range(&from, 2).await.unwrap();
    assert_eq!(verified_headers.len(), 2);

    let height2 = node.request_header_by_height(2).await.unwrap();
    assert_eq!(verified_headers[0], height2);

    let height3 = node.request_header_by_height(3).await.unwrap();
    assert_eq!(verified_headers[1], height3);
}

#[tokio::test]
async fn request_head() {
    let node = new_connected_node().await;

    let genesis = node.request_header_by_height(1).await.unwrap();

    let head1 = node.request_head_header().await.unwrap();
    genesis.verify(&head1).unwrap();

    let head2 = node.request_header_by_height(0).await.unwrap();
    assert!(head1 == head2 || head1.verify(&head2).is_ok());
}

#[tokio::test]
async fn client_server() {
    // Server Node
    let (server_store, mut header_generator) = gen_filled_store(0);
    let server_headers = header_generator.next_many(20);
    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.listeners().await.unwrap();

    // Client node
    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs.clone(),
        ..test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    // request head (with one peer)
    let received_head = client.request_head_header().await.unwrap();
    assert_eq!(server_headers.last().unwrap(), &received_head);

    // request by height
    let received_header_by_height = client.request_header_by_height(10).await.unwrap();
    assert_eq!(server_headers[9], received_header_by_height);

    // request by hash
    let expected_header = &server_headers[15];
    let received_header_by_hash = client
        .request_header_by_hash(&expected_header.hash())
        .await
        .unwrap();
    assert_eq!(expected_header, &received_header_by_hash);

    // request genesis by height
    let received_genesis = client.request_header_by_height(1).await.unwrap();
    assert_eq!(server_headers.first().unwrap(), &received_genesis);

    // request entire store range
    let received_all_headers = client
        .request_verified_headers_range(&received_genesis, 19)
        .await
        .unwrap();
    assert_eq!(server_headers[1..], received_all_headers);

    // reqest more headers than available in store
    // TODO: this reflects _current_ behaviour. once sessions are implemented it'll keep retrying
    // and this test will need to be changed
    let partial_response = client
        .request_verified_headers_range(&received_genesis, 20)
        .await
        .unwrap();
    assert_eq!(partial_response.len(), 19);

    // request unknown hash
    let unstored_header = header_generator.next_of(&server_headers[0]);
    let unexpected_hash = client
        .request_header_by_hash(&unstored_header.hash())
        .await
        .unwrap_err();
    assert!(matches!(
        unexpected_hash,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::HeaderNotFound))
    ));

    // request unknown height
    let unexpected_height = client.request_header_by_height(21).await.unwrap_err();
    assert!(matches!(
        unexpected_height,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::HeaderNotFound))
    ));
}

#[tokio::test]
async fn client_server_invalid_requests() {
    // Server Node
    let server = Node::new(NodeConfig {
        store: gen_filled_store(20).0,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.listeners().await.unwrap();

    // Client node
    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs.clone(),
        ..test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    let none_data = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: None,
            amount: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        none_data,
        P2pError::HeaderEx(HeaderExError::InvalidRequest)
    ));

    let zero_amount = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Origin(5)),
            amount: 0,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        zero_amount,
        P2pError::HeaderEx(HeaderExError::InvalidRequest)
    ));

    let malformed_hash = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Hash(vec![0; 31])),
            amount: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        malformed_hash,
        P2pError::HeaderEx(HeaderExError::InvalidRequest)
    ));
}

#[tokio::test]
async fn head_selection_with_multiple_peers() {
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
            ..listening_test_node_config()
        })
        .await
        .unwrap(),
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listening_test_node_config()
        })
        .await
        .unwrap(),
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listening_test_node_config()
        })
        .await
        .unwrap(),
    ];

    // Server group B, single node with additional headers
    let additional_server_headers = header_generator.next_many(5);
    server_store
        .append_unchecked(additional_server_headers.clone())
        .await
        .unwrap();

    servers.push(
        Node::new(NodeConfig {
            store: server_store.clone(),
            ..listening_test_node_config()
        })
        .await
        .unwrap(),
    );

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;

    let mut server_addrs = vec![];
    for s in &servers {
        server_addrs.extend_from_slice(&s.listeners().await.unwrap()[..]);
    }

    // Client Node
    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    // give client node a sec to breathe, otherwise occiasionally rogue node has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let client_addr = client.listeners().await.unwrap();

    // Rogue node, connects to client so isn't trusted
    let rogue_node = Node::new(NodeConfig {
        store: gen_filled_store(26).0,
        p2p_bootnodes: client_addr.clone(),
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    rogue_node.wait_connected().await.unwrap();
    // small delay needed for client to include rogue_node in head selection process
    sleep(Duration::from_millis(50)).await;

    // client should prefer heighest head received from 2+ peers
    let network_head = client.request_head_header().await.unwrap();
    assert_eq!(common_server_headers.last().unwrap(), &network_head);

    // new node from group B joins, head should go up
    let new_b_node = Node::new(NodeConfig {
        store: server_store.clone(),
        p2p_bootnodes: client_addr,
        ..test_node_config()
    })
    .await
    .unwrap();

    // Head requests are send only to trusted peers, so we add
    // `new_b_node` as trusted.
    let new_b_peer_id = new_b_node.local_peer_id().to_owned();
    client.set_peer_trust(new_b_peer_id, true).await.unwrap();

    new_b_node.wait_connected().await.unwrap();
    // small delay needed for client to include new_b_node in head selection process
    sleep(Duration::from_millis(50)).await;

    // now 2 nodes agree on head with height 25
    let network_head = client.request_head_header().await.unwrap();
    assert_eq!(additional_server_headers.last().unwrap(), &network_head);
}

#[tokio::test]
async fn replaced_header_server_store() {
    // Server node, header at height 11 shouldn't pass verification as it's been tampered with
    let (server_store, mut header_generator) = gen_filled_store(0);
    let mut server_headers = header_generator.next_many(20);
    // replaced header still pases verification and validation against itself
    let replaced_header = header_generator.another_of(&server_headers[10]);
    server_headers[10] = replaced_header.clone();

    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    let tampered_header_in_range = client
        .request_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    let requested_from_tampered_header = client
        .request_verified_headers_range(&replaced_header, 1)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_tampered_header,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    // non-validating requests should still accept responses
    let tampered_header_in_range = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap();
    assert_eq!(tampered_header_in_range, server_headers[7..12]);

    let requested_tampered_header = client
        .request_header_by_hash(&replaced_header.hash())
        .await
        .unwrap();
    assert_eq!(requested_tampered_header, replaced_header);

    let network_head = client.request_head_header().await.unwrap();
    assert_eq!(server_headers.last().unwrap(), &network_head);
}

#[tokio::test]
async fn invalidated_header_server_store() {
    // Server node, header at height 11 shouldn't pass verification as it's been tampered with
    let (server_store, mut header_generator) = gen_filled_store(0);
    let mut server_headers = header_generator.next_many(20);
    invalidate(&mut server_headers[10]);

    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    let invalidated_header_in_range = client
        .request_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        invalidated_header_in_range,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    let requested_from_invalidated_header = client
        .request_verified_headers_range(&server_headers[10], 3)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_invalidated_header,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidRequest))
    ));

    // received ExtendedHeaders are validated during conversion from HeaderResponse
    let tampered_header_in_range = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        P2pError::HeaderEx(HeaderExError::InvalidResponse)
    ));

    let requested_tampered_header = client
        .request_header_by_hash(&server_headers[10].hash())
        .await
        .unwrap_err();
    assert!(matches!(
        requested_tampered_header,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    // requests for non-invalidated headers should still pass
    let valid_header = client.request_header_by_height(10).await.unwrap();
    assert_eq!(server_headers[9], valid_header);
}

#[tokio::test]
async fn unverified_header_server_store() {
    // Server node, header at height 11 shouldn't pass verification as it's been tampered with
    let (server_store, mut header_generator) = gen_filled_store(0);
    let mut server_headers = header_generator.next_many(20);
    unverify(&mut server_headers[10]);

    server_store
        .append_unchecked(server_headers.clone())
        .await
        .unwrap();

    let server = Node::new(NodeConfig {
        store: server_store,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    // give server a sec to breathe, otherwise occiasionally client has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let server_addrs = server.listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootnodes: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.wait_connected().await.unwrap();

    let tampered_header_in_range = client
        .request_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    let requested_from_tampered_header = client
        .request_verified_headers_range(&server_headers[10], 3)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_tampered_header,
        NodeError::P2p(P2pError::HeaderEx(HeaderExError::InvalidResponse))
    ));

    // non-verifying requests should still accept responses
    let tampered_header_in_range = client
        .p2p()
        .header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap();
    assert_eq!(tampered_header_in_range, server_headers[7..12]);

    let requested_tampered_header = client
        .request_header_by_hash(&server_headers[10].hash())
        .await
        .unwrap();
    assert_eq!(requested_tampered_header, server_headers[10]);

    let network_head = client.request_head_header().await.unwrap();
    assert_eq!(server_headers.last().unwrap(), &network_head);
}
