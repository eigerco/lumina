#![cfg(not(target_arch = "wasm32"))]

use celestia_rpc::blobstream::BlobstreamClient;

pub mod utils;

use crate::utils::client::{new_test_client, AuthLevel};

#[tokio::test]
async fn get_data_root_tuple_root() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();

    let result = client
        .get_data_root_tuple_root(4211296, 4211298)
        .await
        .unwrap();

    println!("data root tuple root: {:x?}", result);
}

#[tokio::test]
async fn get_data_root_tuple_inclusion_proof() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();

    let result = client
        .get_data_root_tuple_inclusion_proof(4211299, 4211298, 4211300)
        .await
        .unwrap();

    println!("data root tuple root: {:x?}", result);
}
