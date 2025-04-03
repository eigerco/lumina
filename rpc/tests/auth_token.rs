#![cfg(not(target_arch = "wasm32"))]

use celestia_types::consts::appconsts::AppVersion;
use celestia_types::Blob;

pub mod utils;

use crate::utils::client::{blob_submit, new_test_client_with_url, AuthLevel};
use crate::utils::{random_bytes, random_ns};

// Use node-1 (bridge node) as the RPC URL
const CELESTIA_BRIDGE_RPC_URL: &str = "ws://localhost:36658";

#[tokio::test]
async fn blob_submit_using_bridge_node() {
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data, AppVersion::V2).unwrap();

    // AuthLevel::Skip
    let unauthorized_client = new_test_client_with_url(AuthLevel::Skip, CELESTIA_BRIDGE_RPC_URL).await;
    assert!(unauthorized_client.is_err());

    for auth_level in [AuthLevel::Read, AuthLevel::Write, AuthLevel::Admin] {
        let client = new_test_client_with_url(auth_level, CELESTIA_BRIDGE_RPC_URL)
            .await
            .unwrap();

        match auth_level {
            AuthLevel::Read => {
                blob_submit(&client, &[blob.clone()]).await.unwrap_err();
            }
            _ => {
                blob_submit(&client, &[blob.clone()]).await.unwrap();
            }
        }
    }
}
