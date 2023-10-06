use std::time::Duration;

use celestia_node::{
    node::{Node, NodeConfig},
    p2p::{ExchangeError, P2pError},
    store::Store,
    test_utils::{gen_filled_store, listening_test_node_config, test_node_config},
};
use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::test_utils::{invalidate, unverify};
use tokio::time::sleep;

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

    // request by height
    let received_header_by_height = client.p2p().get_header_by_height(10).await.unwrap();
    assert_eq!(server_headers[9], received_header_by_height);

    // request by hash
    let expected_header = &server_headers[15];
    let received_header_by_hash = client
        .p2p()
        .get_header(expected_header.hash())
        .await
        .unwrap();
    assert_eq!(expected_header, &received_header_by_hash);

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
    // TODO: this reflects _current_ behaviour. once sessions are implemented it'll keep retrying
    // and this test will need to be changed
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
    let server_addrs = server.p2p().listeners().await.unwrap();

    // Client node
    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs.clone(),
        ..test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    let none_data = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: None,
            amount: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        none_data,
        P2pError::Exchange(ExchangeError::InvalidRequest)
    ));

    let zero_amount = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(5)),
            amount: 0,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        zero_amount,
        P2pError::Exchange(ExchangeError::InvalidRequest)
    ));

    let malformed_hash = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Hash(vec![0; 31])),
            amount: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        malformed_hash,
        P2pError::Exchange(ExchangeError::InvalidRequest)
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
    for s in servers {
        server_addrs.extend_from_slice(&s.p2p().listeners().await.unwrap()[..]);
    }

    // Client Node
    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    // give client node a sec to breathe, otherwise occiasionally rogue node has problems with connecting
    sleep(Duration::from_millis(100)).await;
    let client_addr = client.p2p().listeners().await.unwrap();

    // Rogue node, connects to client so isn't trusted
    let rogue_node = Node::new(NodeConfig {
        store: gen_filled_store(26).0,
        p2p_bootstrap_peers: client_addr.clone(),
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    rogue_node.p2p().wait_connected().await.unwrap();
    // small delay needed for client to include rogue_node in head selection process
    sleep(Duration::from_millis(50)).await;

    // client should prefer heighest head received from 2+ peers
    let network_head = client.p2p().get_head_header().await.unwrap();
    assert_eq!(common_server_headers.last().unwrap(), &network_head);

    // new node from group B joins, head should go up
    let new_b_node = Node::new(NodeConfig {
        store: server_store.clone(),
        p2p_bootstrap_peers: client_addr,
        ..test_node_config()
    })
    .await
    .unwrap();

    new_b_node.p2p().wait_connected().await.unwrap();
    // small delay needed for client to include new_b_node in head selection process
    sleep(Duration::from_millis(50)).await;

    // now 2 nodes agree on head with height 25
    let network_head = client.p2p().get_head_header().await.unwrap();
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
    let server_addrs = server.p2p().listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    let tampered_header_in_range = client
        .p2p()
        .get_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    let requested_from_tampered_header = client
        .p2p()
        .get_verified_headers_range(&replaced_header, 1)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_tampered_header,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    // non-validating requests should still accept responses
    let tampered_header_in_range = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap();
    assert_eq!(tampered_header_in_range, server_headers[7..12]);

    let requested_tampered_header = client
        .p2p()
        .get_header(replaced_header.hash())
        .await
        .unwrap();
    assert_eq!(requested_tampered_header, replaced_header);

    let network_head = client.p2p().get_head_header().await.unwrap();
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
    let server_addrs = server.p2p().listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    let invalidated_header_in_range = client
        .p2p()
        .get_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        invalidated_header_in_range,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    let requested_from_invalidated_header = client
        .p2p()
        .get_verified_headers_range(&server_headers[10], 3)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_invalidated_header,
        P2pError::Exchange(ExchangeError::InvalidRequest)
    ));

    // received ExtendedHeaders are validated during conversion from HeaderResponse
    let tampered_header_in_range = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    let requested_tampered_header = client
        .p2p()
        .get_header(server_headers[10].hash())
        .await
        .unwrap_err();
    assert!(matches!(
        requested_tampered_header,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    // requests for non-invalidated headers should still pass
    let valid_header = client.p2p().get_header_by_height(10).await.unwrap();
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
    let server_addrs = server.p2p().listeners().await.unwrap();

    let client = Node::new(NodeConfig {
        p2p_bootstrap_peers: server_addrs,
        ..listening_test_node_config()
    })
    .await
    .unwrap();

    client.p2p().wait_connected().await.unwrap();

    let tampered_header_in_range = client
        .p2p()
        .get_verified_headers_range(&server_headers[9], 5)
        .await
        .unwrap_err();
    assert!(matches!(
        tampered_header_in_range,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    let requested_from_tampered_header = client
        .p2p()
        .get_verified_headers_range(&server_headers[10], 3)
        .await
        .unwrap_err();
    assert!(matches!(
        requested_from_tampered_header,
        P2pError::Exchange(ExchangeError::InvalidResponse)
    ));

    // non-verifying requests should still accept responses
    let tampered_header_in_range = client
        .p2p()
        .exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(8)),
            amount: 5,
        })
        .await
        .unwrap();
    assert_eq!(tampered_header_in_range, server_headers[7..12]);

    let requested_tampered_header = client
        .p2p()
        .get_header(server_headers[10].hash())
        .await
        .unwrap();
    assert_eq!(requested_tampered_header, server_headers[10]);

    let network_head = client.p2p().get_head_header().await.unwrap();
    assert_eq!(server_headers.last().unwrap(), &network_head);
}
