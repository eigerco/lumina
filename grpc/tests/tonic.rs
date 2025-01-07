#![cfg(not(target_arch = "wasm32"))]

use celestia_grpc::types::auth::Account;
use celestia_grpc::types::tx::sign_tx;
use celestia_proto::cosmos::tx::v1beta1::BroadcastMode;
use celestia_types::blob::MsgPayForBlobs;
use celestia_types::nmt::Namespace;
use celestia_types::{AppVersion, Blob};

pub mod utils;

use crate::utils::{load_account, new_test_client};

const BRIDGE_0_ACCOUNT_DATA: &str = "../ci/credentials/bridge-0";

#[tokio::test]
async fn get_min_gas_price() {
    let mut client = new_test_client().await.unwrap();
    let gas_price = client.get_min_gas_price().await.unwrap();
    assert!(gas_price > 0.0);
}

#[tokio::test]
async fn get_blob_params() {
    let mut client = new_test_client().await.unwrap();
    let params = client.get_blob_params().await.unwrap();
    assert!(params.gas_per_blob_byte > 0);
    assert!(params.gov_max_square_size > 0);
}

#[tokio::test]
async fn get_auth_params() {
    let mut client = new_test_client().await.unwrap();
    let params = client.get_auth_params().await.unwrap();
    assert!(params.max_memo_characters > 0);
    assert!(params.tx_sig_limit > 0);
    assert!(params.tx_size_cost_per_byte > 0);
    assert!(params.sig_verify_cost_ed25519 > 0);
    assert!(params.sig_verify_cost_secp256k1 > 0);
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
        Account::Base(acct) => acct.address.clone(),
        Account::Module(acct) => acct.base_account.as_ref().unwrap().address.clone(),
    };

    let account = client.get_account(&address).await.unwrap();

    assert_eq!(&account, first_account);
}

#[tokio::test]
async fn submit_blob() {
    let mut client = new_test_client().await.unwrap();

    let account_credentials = load_account(BRIDGE_0_ACCOUNT_DATA);
    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "Hello, World!".into(), AppVersion::V3).unwrap()];
    let chain_id = "private".to_string();
    let account = client
        .get_account(&account_credentials.address)
        .await
        .unwrap();
    // gas and fees are overestimated for simplicity
    let gas_limit = 100000;
    let fee = 5000;

    let msg_pay_for_blobs = MsgPayForBlobs::new(&blobs, account_credentials.address).unwrap();

    let tx = sign_tx(
        msg_pay_for_blobs.into(),
        chain_id,
        account.base_account_ref().unwrap(),
        account_credentials.verifying_key,
        account_credentials.signing_key,
        gas_limit,
        fee,
    );

    let response = client
        .broadcast_blob_tx(tx, blobs, BroadcastMode::Sync)
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    let _submitted_tx = client
        .get_tx(response.txhash)
        .await
        .expect("get to be successful");
}
