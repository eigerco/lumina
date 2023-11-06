use celestia_rpc::prelude::*;

pub mod utils;

use crate::utils::client::{new_test_client, AuthLevel};

#[tokio::test]
async fn local_head() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let local_head = client.header_local_head().await.unwrap();

    let head_height = local_head.height();
    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let adjacent_header = client
        .header_get_by_height(head_height.value() - 1)
        .await
        .unwrap();

    local_head.validate().unwrap();
    genesis_header.verify(&local_head).unwrap();
    adjacent_header.verify(&local_head).unwrap();
}

#[tokio::test]
async fn get_by_height() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let second_header = client.header_get_by_height(2).await.unwrap();

    genesis_header.validate().unwrap();
    second_header.validate().unwrap();
}

#[tokio::test]
async fn get_by_height_non_existent() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    client.header_get_by_height(999_999_999).await.unwrap_err();
}

#[tokio::test]
async fn get_by_hash() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let genesis_header2 = client
        .header_get_by_hash(genesis_header.hash())
        .await
        .unwrap();

    assert_eq!(genesis_header, genesis_header2);
}

#[tokio::test]
async fn get_range_by_height() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let second_header = client.header_get_by_height(2).await.unwrap();

    let headers = client
        .header_get_range_by_height(&genesis_header, 3)
        .await
        .unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(second_header, headers[0]);
}

#[tokio::test]
async fn network_head() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let network_head = client.header_network_head().await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let adjacent_header = client
        .header_get_by_height(network_head.height().value() - 1)
        .await
        .unwrap();

    network_head.validate().unwrap();
    genesis_header.verify(&network_head).unwrap();
    adjacent_header.verify(&network_head).unwrap();
}

#[tokio::test]
async fn subscribe() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();

    let mut incoming_headers = client.header_subscribe().await.unwrap();
    let header1 = incoming_headers.next().await.unwrap().unwrap();
    let header2 = incoming_headers.next().await.unwrap().unwrap();

    genesis_header.verify(&header1).unwrap();
    header1.verify(&header2).unwrap();
}

#[tokio::test]
async fn sync_state() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let state1 = client.header_sync_state().await.unwrap();

    client
        .header_wait_for_height(state1.height + 1)
        .await
        .unwrap();

    let state2 = client.header_sync_state().await.unwrap();
    assert!(state2.height > state1.height);
}
