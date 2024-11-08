#![cfg(not(target_arch = "wasm32"))]

use celestia_grpc::types::auth::Account;

pub mod utils;

use crate::utils::new_test_client;

#[tokio::test]
async fn get_min_gas_price() {
    let mut client = new_test_client().await.unwrap();
    let gas_price = client.get_min_gas_price().await.unwrap();
    assert!(gas_price > 0.0);
}

#[tokio::test]
async fn get_block() {
    let mut client = new_test_client().await.unwrap();

    let latest_block = client.get_latest_block().await.unwrap();
    let height = latest_block.header.height.value() as i64;

    let block = client.get_block_by_height(height).await.unwrap();
    assert_eq!(block.header, latest_block.header);
}

#[tokio::test]
async fn get_account() {
    let mut client = new_test_client().await.unwrap();

    let accounts = client.get_accounts().await.unwrap();

    let first_account = accounts.first().expect("account to exist");

    let address = match first_account {
        Account::Base(acct) => acct.address.to_string(),
        Account::Module(acct) => acct.base_account.as_ref().unwrap().address.to_string(),
        _ => unimplemented!("unknown account type"),
    };

    let account = client.get_account(address).await.unwrap();

    assert_eq!(&account, first_account);
}
