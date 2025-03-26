#![cfg(not(target_arch = "wasm32"))]

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use celestia_rpc::{blobstream::BlobstreamClient, HeaderClient};
use celestia_types::hash::Hash;
pub mod utils;

use crate::utils::client::{new_test_client, AuthLevel};

#[tokio::test]
async fn get_data_root_tuple_root_and_proof() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();

    let network_head = client.header_network_head().await.unwrap();

    let network_height = network_head.height().value();

    let start_height = network_height - 2;
    let target_height = network_height - 1;
    // Use network head for these tests
    let tuple_root = client
        .blobstream_get_data_root_tuple_root(start_height, network_height)
        .await
        .unwrap();

    let proof = client
        .blobstream_get_data_root_tuple_inclusion_proof(target_height, start_height, network_height)
        .await
        .unwrap();

    let data_root = client
        .header_get_by_height(target_height)
        .await
        .unwrap()
        .dah
        .hash();

    let leaf = encode_data_root_tuple(target_height, &data_root);

    let mut root = [0u8; 32];
    root.copy_from_slice(&BASE64.decode(tuple_root.as_ref()).unwrap());

    proof
        .verify(leaf, root)
        .expect("failed to verify data root proof");
}

pub fn encode_data_root_tuple(height: u64, data_root: &Hash) -> Vec<u8> {
    // Create the result vector with 64 bytes capacity
    let mut result = Vec::with_capacity(64);

    // Pad the height to 32 bytes (convert to big-endian and pad with zeros)
    let height_bytes = height.to_be_bytes();

    // Add leading zeros (24 bytes of padding)
    result.extend_from_slice(&[0u8; 24]);

    // Add the 8-byte height
    result.extend_from_slice(&height_bytes);

    // Add the 32-byte data root
    result.extend_from_slice(data_root.as_bytes());

    result
}
