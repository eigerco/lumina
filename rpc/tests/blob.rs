#![cfg(not(target_arch = "wasm32"))]

use std::cmp::Ordering;
use std::time::Duration;

use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::prelude::*;
use celestia_types::consts::appconsts::AppVersion;
use celestia_types::{Blob, Commitment};
use jsonrpsee::core::client::Subscription;

pub mod utils;

use crate::utils::client::{blob_submit, new_test_client, AuthLevel};
use crate::utils::{random_bytes, random_bytes_array, random_ns};

#[tokio::test]
async fn blob_submit_and_get() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
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

#[tokio::test]
async fn blob_submit_and_get_all() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespaces = &[random_ns(), random_ns()];

    let blobs = &[
        Blob::new(namespaces[0], random_bytes(5), AppVersion::V2).unwrap(),
        Blob::new(namespaces[1], random_bytes(15), AppVersion::V2).unwrap(),
    ];

    let submitted_height = blob_submit(&client, &blobs[..]).await.unwrap();

    let received_blobs = client
        .blob_get_all(submitted_height, namespaces)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(received_blobs.len(), 2);

    for (idx, (blob, received_blob)) in blobs.iter().zip(received_blobs.iter()).enumerate() {
        let namespace = namespaces[idx];

        received_blob.validate(AppVersion::V2).unwrap();
        assert_blob_equal_to_sent(received_blob, blob);

        let proofs = client
            .blob_get_proof(submitted_height, namespace, blob.commitment)
            .await
            .unwrap();

        assert_eq!(proofs.len(), 1);
    }
}

#[tokio::test]
async fn blob_submit_and_get_large() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024 * 1024);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    // It takes a while for a node to process large blob
    // so we need to wait a bit
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let received_blob = client
        .blob_get(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    blob.validate(AppVersion::V2).unwrap();
    assert_blob_equal_to_sent(&received_blob, &blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    assert!(proofs.len() > 1);
    // TODO: can't verify the proofs until we have the end index inside the proof
    //       because without it we can't know how many shares there are in each row
}

#[tokio::test]
async fn blob_subscribe() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();

    let mut incoming_blobs = client.blob_subscribe(namespace).await.unwrap();

    // nothing was submitted
    let received_blobs = incoming_blobs.next().await.unwrap().unwrap();
    assert!(received_blobs.blobs.is_none());

    // submit and receive blob
    let blob = Blob::new(namespace, random_bytes(10), AppVersion::V2).unwrap();
    let current_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    let received = blobs_at_height(current_height, &mut incoming_blobs).await;
    assert_eq!(received.len(), 1);
    assert_blob_equal_to_sent(&received[0], &blob);

    // submit blob to another ns
    let blob_another_ns = Blob::new(random_ns(), random_bytes(10), AppVersion::V2).unwrap();
    let current_height = blob_submit(&client, &[blob_another_ns]).await.unwrap();

    let received = blobs_at_height(current_height, &mut incoming_blobs).await;
    assert!(received.is_empty());

    // submit and receive few blobs
    let blob1 = Blob::new(namespace, random_bytes(10), AppVersion::V2).unwrap();
    let blob2 = Blob::new(random_ns(), random_bytes(10), AppVersion::V2).unwrap(); // different ns
    let blob3 = Blob::new(namespace, random_bytes(10), AppVersion::V2).unwrap();
    let current_height = blob_submit(&client, &[blob1.clone(), blob2, blob3.clone()])
        .await
        .unwrap();

    let received = blobs_at_height(current_height, &mut incoming_blobs).await;
    assert_eq!(received.len(), 2);
    assert_blob_equal_to_sent(&received[0], &blob1);
    assert_blob_equal_to_sent(&received[1], &blob3);
}

#[tokio::test]
async fn blob_submit_too_large() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5 * 1024 * 1024);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();

    blob_submit(&client, &[blob]).await.unwrap_err();
}

#[tokio::test]
async fn blob_get_get_proof_wrong_ns() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    client
        .blob_get(submitted_height, random_ns(), blob.commitment)
        .await
        .unwrap_err();

    client
        .blob_get_proof(submitted_height, random_ns(), blob.commitment)
        .await
        .unwrap_err();
}

#[tokio::test]
async fn blob_get_get_proof_wrong_commitment() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();
    let commitment = Commitment::new(random_bytes_array());

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    client
        .blob_get(submitted_height, namespace, commitment)
        .await
        .unwrap_err();

    client
        .blob_get_proof(submitted_height, namespace, commitment)
        .await
        .unwrap_err();
}

#[tokio::test]
async fn blob_get_all_with_no_blobs() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let blobs = client.blob_get_all(3, &[random_ns()]).await.unwrap();

    assert!(blobs.is_none());
}

// Skips blobs at height subscription until provided height is reached, then return blobs for the height
async fn blobs_at_height(height: u64, sub: &mut Subscription<BlobsAtHeight>) -> Vec<Blob> {
    while let Some(received) = sub.next().await {
        let received = received.unwrap();
        match received.height.cmp(&height) {
            Ordering::Less => continue,
            Ordering::Equal => return received.blobs.unwrap_or_default(),
            Ordering::Greater => panic!("height {height} missed"),
        }
    }
    panic!("subscription error");
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
