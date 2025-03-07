#![cfg(not(target_arch = "wasm32"))]

use celestia_rpc::prelude::*;

pub mod utils;

use crate::utils::client::{new_test_client, AuthLevel};

#[tokio::test]
async fn das_sampling_stats() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let stats1 = client.das_sampling_stats().await.unwrap();

    client
        .header_wait_for_height(stats1.network_head_height + 1)
        .await
        .unwrap();

    let stats2 = client.das_sampling_stats().await.unwrap();

    assert!(stats2.head_of_sampled_chain >= stats1.head_of_sampled_chain);
}

#[tokio::test]
async fn das_wait_catch_up() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    client.das_wait_catch_up().await.unwrap();

    let stats = client.das_sampling_stats().await.unwrap();

    assert!(stats.catch_up_done);
}
