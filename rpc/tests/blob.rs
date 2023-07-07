use std::time::Duration;

use celestia_rpc::prelude::*;
use celestia_types::{Blob, Commitment};

pub mod utils;

use crate::utils::client::blob_submit;
use crate::utils::{random_bytes, random_bytes_array, random_ns, test_client, AuthLevel};

#[tokio::test]
async fn blob_submit_and_get() {
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    let received_blob = client
        .blob_get(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    received_blob.validate().unwrap();
    assert_eq!(received_blob, blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    assert_eq!(proofs.len(), 1);
}

#[tokio::test]
async fn blob_submit_and_get_all() {
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespaces = &[random_ns(), random_ns()];

    let blobs = &[
        Blob::new(namespaces[0], random_bytes(5)).unwrap(),
        Blob::new(namespaces[0], random_bytes(15)).unwrap(),
        Blob::new(namespaces[1], random_bytes(25)).unwrap(),
    ];

    let submitted_height = blob_submit(&client, &blobs[..]).await.unwrap();

    let received_blobs = client
        .blob_get_all(submitted_height, namespaces)
        .await
        .unwrap();

    assert_eq!(received_blobs.len(), 3);

    for (idx, (blob, received_blob)) in blobs.iter().zip(received_blobs.iter()).enumerate() {
        let namespace = namespaces[idx / 2];

        received_blob.validate().unwrap();
        assert_eq!(received_blob, blob);

        let proofs = client
            .blob_get_proof(submitted_height, namespace, blob.commitment)
            .await
            .unwrap();

        assert_eq!(proofs.len(), 1);
    }
}

#[tokio::test]
async fn blob_submit_and_get_large() {
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(1024 * 1024);
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = blob_submit(&client, &[blob.clone()]).await.unwrap();

    // It takes a while for a node to process large blob
    // so we need to wait a bit
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let received_blob = client
        .blob_get(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    blob.validate().unwrap();
    assert_eq!(received_blob, blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, blob.commitment)
        .await
        .unwrap();

    assert!(proofs.len() > 1);
}

#[tokio::test]
async fn blob_submit_too_large() {
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5 * 1024 * 1024);
    let blob = Blob::new(namespace, data).unwrap();

    blob_submit(&client, &[blob]).await.unwrap_err();
}

#[tokio::test]
async fn blob_get_get_proof_wrong_ns() {
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data).unwrap();

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
    let client = test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data).unwrap();
    let commitment = Commitment(random_bytes_array());

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
