use celestia_rpc::prelude::*;

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
