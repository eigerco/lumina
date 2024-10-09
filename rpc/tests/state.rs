#![cfg(not(target_arch = "wasm32"))]

use crate::utils::{random_bytes, random_ns};
use celestia_rpc::prelude::*;
use celestia_types::{Blob, TxConfig};

pub mod utils;

use crate::utils::client::{new_test_client, AuthLevel};

#[tokio::test]
async fn account_address() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();
    let _addr = client.state_account_address().await.unwrap();
}

#[tokio::test]
async fn balance() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();
    let balance = client.state_balance().await.unwrap();
    assert_eq!(balance.denom, "utia");
}

#[tokio::test]
async fn balance_for_address() {
    let client = new_test_client(AuthLevel::Read).await.unwrap();

    let my_addr = client.state_account_address().await.unwrap();
    let my_balance = client.state_balance().await.unwrap();

    let balance = client.state_balance_for_address(&my_addr).await.unwrap();
    assert_eq!(my_balance, balance);
}

#[tokio::test]
async fn submit_pay_for_blob() {
    let client = new_test_client(AuthLevel::Write).await.unwrap();
    let namespace = random_ns();
    let data = random_bytes(5);
    let blob = Blob::new(namespace, data).unwrap();

    let tx_response = client
        .state_submit_pay_for_blob(&[blob.clone().into()], TxConfig::default())
        .await
        .unwrap();

    let received_blob = client
        .blob_get(tx_response.height as u64, namespace, blob.commitment)
        .await
        .unwrap();

    received_blob.validate().unwrap();
    assert_eq!(received_blob.data, blob.data);
}
