#![cfg(not(target_arch = "wasm32"))]

use celestia_rpc::prelude::*;
use celestia_types::consts::appconsts::AppVersion;
use celestia_types::Blob;

pub mod utils;

use crate::utils::client::{blob_submit, new_test_client, AuthLevel};
use crate::utils::{random_bytes, random_ns};

// Use node-1 (bridge node) as the RPC URL
const CELESTIA_BRIDGE_RPC_URL: &str = "ws://localhost:36658";

#[tokio::test]
async fn blob_submit_and_get_using_bridge_node() {
    let client = new_test_client(AuthLevel::Write, Some(CELESTIA_BRIDGE_RPC_URL))
        .await
        .unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;
    let root_hash = dah.row_root(0).unwrap();

    let received_blob = client
        .blob_get(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    received_blob.validate(AppVersion::V2).unwrap();
    assert_blob_equal_to_sent(&received_blob, &blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    assert_eq!(proofs.len(), 1);

    let leaves = blob.to_shares().unwrap();

    proofs[0]
        .verify_complete_namespace(&root_hash, &leaves, namespace.into())
        .unwrap();
}

/// Blobs received from chain have index field set, so to
/// compare if they are equal to the ones we sent, we need
/// to overwrite the index field with received one.
#[track_caller]
fn assert_blob_equal_to_sent(received: &Blob, sent: &Blob) {
    let mut sent = sent.clone();
    sent.index = received.index;
    assert_eq!(&sent, received);
}
