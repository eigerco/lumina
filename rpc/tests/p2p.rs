#![cfg(all(feature = "p2p", not(target_arch = "wasm32")))]

use celestia_rpc::p2p_types as p2p;
use celestia_rpc::prelude::*;
use libp2p::{PeerId, identity};
use tokio::time::{Duration, sleep};

pub mod utils;

use crate::utils::client::{AuthLevel, new_test_client};

#[tokio::test]
async fn info_test() {
    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    client.p2p_info().await.expect("Failed to get node info");
}

#[tokio::test]
async fn add_remove_peer_test() {
    // add and then remove a peer, testing outputs from `p2p.Peers` and `p2p.Connectedness`
    let addr_info = utils::tiny_node::start_tiny_node()
        .await
        .expect("failed to spin up second node");
    let client = new_test_client(AuthLevel::Skip).await.unwrap();

    let initial_peers = client
        .p2p_peers()
        .await
        .expect("failed to get initial peer list");
    assert!(!initial_peers.contains(&addr_info.id));

    let connected_to_peer = client
        .p2p_connectedness(&addr_info.id)
        .await
        .expect("failed to check initial connection to peer");
    assert_eq!(connected_to_peer, p2p::Connectedness::NotConnected);

    client
        .p2p_connect(&addr_info)
        .await
        .expect("request to connect to second node failed");
    rpc_call_delay().await;

    let peers = client
        .p2p_peers()
        .await
        .expect("failed to get peer list after connect request");
    assert!(peers.contains(&addr_info.id));

    let connected_to_peer = client
        .p2p_connectedness(&addr_info.id)
        .await
        .expect("failed to check connection to peer after connect request");
    assert_eq!(connected_to_peer, p2p::Connectedness::Connected);

    client
        .p2p_close_peer(&addr_info.id)
        .await
        .expect("Failed to close peer");
    rpc_call_delay().await;

    let final_peers = client
        .p2p_peers()
        .await
        .expect("failed to get peer list after close peer request");
    assert!(!final_peers.contains(&addr_info.id));
}

#[tokio::test]
async fn protect_unprotect_test() {
    // check whether reported protect status reacts correctly to protect/unprotect requests and
    // whether node takes tag into the account

    const PROTECT_TAG: &str = "test-tag";
    const ANOTHER_PROTECT_TAG: &str = "test-tag-2";

    let addr_info = utils::tiny_node::start_tiny_node()
        .await
        .expect("failed to spin up second node");
    let client = new_test_client(AuthLevel::Skip).await.unwrap();

    client
        .p2p_connect(&addr_info)
        .await
        .expect("request to connect to second node failed");
    rpc_call_delay().await;

    let is_protected = client
        .p2p_is_protected(&addr_info.id, PROTECT_TAG)
        .await
        .expect("failed to check initial protect status");
    assert!(!is_protected);

    client
        .p2p_protect(&addr_info.id, PROTECT_TAG)
        .await
        .expect("protect request failed");
    rpc_call_delay().await;

    let is_protected = client
        .p2p_is_protected(&addr_info.id, PROTECT_TAG)
        .await
        .expect("failed to check protect status after protect request");
    assert!(is_protected);

    let is_protected_another_tag = client
        .p2p_is_protected(&addr_info.id, ANOTHER_PROTECT_TAG)
        .await
        .expect("failed to check protect status for another tag after protect request");
    assert!(!is_protected_another_tag);

    client
        .p2p_unprotect(&addr_info.id, PROTECT_TAG)
        .await
        .expect("unprotect request failed");
    rpc_call_delay().await;

    let is_protected = client
        .p2p_is_protected(&addr_info.id, PROTECT_TAG)
        .await
        .expect("failed to check protect status after unprotect reqest");
    assert!(!is_protected);
}

#[tokio::test]
async fn peer_block_unblock_test() {
    let addr_info = utils::tiny_node::start_tiny_node()
        .await
        .expect("failed to spin up second node");
    let client = new_test_client(AuthLevel::Skip).await.unwrap();

    let blocked_peers = client
        .p2p_list_blocked_peers()
        .await
        .expect("failed to get blocked peer list");
    assert!(!blocked_peers.contains(&addr_info.id));

    client
        .p2p_block_peer(&addr_info.id)
        .await
        .expect("failed to block peer");
    rpc_call_delay().await;

    let blocked_peers = client
        .p2p_list_blocked_peers()
        .await
        .expect("failed to get blocked peer list");
    assert!(blocked_peers.contains(&addr_info.id));

    client
        .p2p_unblock_peer(&addr_info.id)
        .await
        .expect("failed to block peer");
    rpc_call_delay().await;

    let blocked_peers = client
        .p2p_list_blocked_peers()
        .await
        .expect("failed to get blocked peer list");
    assert!(!blocked_peers.contains(&addr_info.id));
}

#[tokio::test]
async fn bandwidth_stats_test() {
    // just check whether we can get the data without error, node could have been running any
    // amount of time, so any value should be valid.
    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    client
        .p2p_bandwidth_stats()
        .await
        .expect("failed to get bandwidth stats");
}

#[tokio::test]
async fn bandwidth_for_peer_test() {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = p2p::PeerId(PeerId::from(local_key.public()));

    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    let stats = client
        .p2p_bandwidth_for_peer(&local_peer_id)
        .await
        .expect("failed to get bandwidth stats for peer");

    // where should be no data exchanged with peer that we're not connected to
    assert_eq!(stats.total_in, 0.0);
    assert_eq!(stats.total_out, 0.0);
    assert_eq!(stats.rate_in, 0.0);
    assert_eq!(stats.rate_out, 0.0);
}

#[tokio::test]
async fn bandwidth_for_protocol_test() {
    let client = new_test_client(AuthLevel::Skip).await.unwrap();

    // query for nonsense protocol name so that we get all zeros in response
    // until we have better way of inducing traffic
    let stats = client
        .p2p_bandwidth_for_protocol("/foo/bar")
        .await
        .expect("failed to get bandwidth stats");
    assert_eq!(stats.total_in, 0.0);
    assert_eq!(stats.total_out, 0.0);
    assert_eq!(stats.rate_in, 0.0);
    assert_eq!(stats.rate_out, 0.0);
}

#[tokio::test]
async fn nat_status_test() {
    // just query for status and make sure no errors happen, since any value is potentially correct
    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    let _ = client
        .p2p_nat_status()
        .await
        .expect("failed to query NAT status");
}

#[tokio::test]
async fn peer_info_test() {
    let addr_info = utils::tiny_node::start_tiny_node()
        .await
        .expect("failed to spin up second node");
    let client = new_test_client(AuthLevel::Skip).await.unwrap();

    client
        .p2p_connect(&addr_info)
        .await
        .expect("request to connect to second node failed");
    rpc_call_delay().await;

    let connectedness = client
        .p2p_connectedness(&addr_info.id)
        .await
        .expect("failed to check connection to peer after connect request");
    assert_eq!(connectedness, p2p::Connectedness::Connected);

    let peer_info = client
        .p2p_peer_info(&addr_info.id)
        .await
        .expect("failed to get peer info");

    assert_eq!(addr_info.id, peer_info.id);
}

#[tokio::test]
async fn pub_sub_peers_test() {
    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    let peers = client
        .p2p_pub_sub_peers("topic")
        .await
        .expect("failed to get topic peers");

    assert!(peers.is_none())
}

#[tokio::test]
async fn resource_state_test() {
    // cannot really test values here, just make sure it deserializes correctly
    let client = new_test_client(AuthLevel::Skip).await.unwrap();
    client
        .p2p_resource_state()
        .await
        .expect("failed to get resource state");
}

async fn rpc_call_delay() {
    // delay for RPC calls like connect/close to let node finish the operation before we query it
    // again. Below 150 ms I start getting intermittent failures.
    sleep(Duration::from_millis(500)).await;
}
