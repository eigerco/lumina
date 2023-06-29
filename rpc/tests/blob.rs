use celestia_rpc::prelude::*;
use jsonrpsee::http_client::HttpClient;

mod utils;

use utils::{random_ns, test_client, AuthLevel};

async fn test_blob_submit_and_get(client: &HttpClient) {
    let namespace = random_ns();
    let data = b"foo".to_vec();
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = client.blob_submit(&[blob.clone()]).await.unwrap();

    let received_blob = client
        .blob_get(submitted_height, namespace, &blob.commitment)
        .await
        .unwrap();

    blob.validate().unwrap();
    assert_eq!(received_blob, blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, &blob.commitment)
        .await
        .unwrap();

    assert_eq!(proofs.len(), 1);
}

async fn test_blob_submit_and_get_large(client: &HttpClient) {
    let namespace = random_ns();
    let data = vec![0xff; 1024 * 1024];
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = client.blob_submit(&[blob.clone()]).await.unwrap();

    let received_blob = client
        .blob_get(submitted_height, namespace, &blob.commitment)
        .await
        .unwrap();

    blob.validate().unwrap();
    assert_eq!(received_blob, blob);

    let proofs = client
        .blob_get_proof(submitted_height, namespace, &blob.commitment)
        .await
        .unwrap();

    assert!(proofs.len() > 1);
}

async fn test_blob_submit_too_large(client: &HttpClient) {
    let namespace = random_ns();
    let data = vec![0xff; 5 * 1024 * 1024];
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = client.blob_submit(&[blob.clone()]).await;
    submitted_height.unwrap_err();
}

#[tokio::test]
async fn blob_api() {
    let client = test_client(AuthLevel::Write).unwrap();

    // minimum 2 blocks
    client.header_wait_for_height(2).await.unwrap();

    test_blob_submit_and_get(&client).await;
    test_blob_submit_and_get_large(&client).await;
    test_blob_submit_too_large(&client).await;
}
