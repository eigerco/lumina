use celestia_rpc::prelude::*;
use jsonrpsee::http_client::HttpClient;

mod utils;

use utils::{test_client, AuthLevel};

async fn test_local_head(client: &HttpClient) {
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

async fn test_get_by_height(client: &HttpClient) {
    let genesis_header = client.header_get_by_height(1).await.unwrap();
    let second_header = client.header_get_by_height(2).await.unwrap();

    genesis_header.validate().unwrap();
    second_header.validate().unwrap();
}

async fn test_get_by_height_non_existent(client: &HttpClient) {
    client.header_get_by_height(999_999_999).await.unwrap_err();
}

#[tokio::test]
async fn header_api() {
    let client = test_client(AuthLevel::Read).unwrap();

    // minimum 3 blocks
    client.header_wait_for_height(3).await.unwrap();

    test_local_head(&client).await;
    test_get_by_height(&client).await;
    test_get_by_height_non_existent(&client).await;
}
