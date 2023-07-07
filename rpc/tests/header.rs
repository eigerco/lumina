use celestia_rpc::prelude::*;

pub mod utils;

use crate::utils::{test_client, AuthLevel};

#[tokio::test]
async fn local_head() {
    let client = test_client(AuthLevel::Read).await.unwrap();

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
    let client = test_client(AuthLevel::Read).await.unwrap();

    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let second_header = client.header_get_by_height(2).await.unwrap();

    genesis_header.validate().unwrap();
    second_header.validate().unwrap();
}

#[tokio::test]
async fn get_by_height_non_existent() {
    let client = test_client(AuthLevel::Read).await.unwrap();

    client.header_get_by_height(999_999_999).await.unwrap_err();
}
