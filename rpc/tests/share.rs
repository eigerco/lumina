use celestia_rpc::prelude::*;
use jsonrpsee::http_client::HttpClient;

mod utils;

use utils::{random_ns, test_client, AuthLevel};

async fn test_get_shares_by_namespace(client: &HttpClient) {
    let namespace = random_ns();
    let data = vec![0xff; 1024];
    let blob = Blob::new(namespace, data).unwrap();

    let submitted_height = client.blob_submit(&[blob.clone()]).await.unwrap();

    let dah = client
        .header_get_by_height(submitted_height)
        .await
        .unwrap()
        .dah;

    let shares = client
        .share_get_shares_by_namespace(&dah, namespace)
        .await
        .unwrap();

    println!("{shares:?}");
    panic!();
}

#[tokio::test]
async fn share_api() {
    let client = test_client(AuthLevel::Write).unwrap();

    // minimum 2 blocks
    client.header_wait_for_height(2).await.unwrap();

    test_get_shares_by_namespace(&client).await;
}
