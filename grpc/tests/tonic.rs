use std::sync::Arc;

use celestia_grpc::{Error, TxConfig};
use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_types::nmt::Namespace;
use celestia_types::state::{Coin, ErrorCode};
use celestia_types::{AppVersion, Blob};
use utils::{load_account, TestAccount};

pub mod utils;

use crate::utils::{new_grpc_client, new_tx_client, spawn};

#[cfg(not(target_arch = "wasm32"))]
use tokio::test as async_test;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as async_test;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[async_test]
async fn get_auth_params() {
    let client = new_grpc_client();
    let params = client.get_auth_params().await.unwrap();
    assert!(params.max_memo_characters > 0);
    assert!(params.tx_sig_limit > 0);
    assert!(params.tx_size_cost_per_byte > 0);
    assert!(params.sig_verify_cost_ed25519 > 0);
    assert!(params.sig_verify_cost_secp256k1 > 0);
}

#[async_test]
async fn get_account() {
    let client = new_grpc_client();

    let accounts = client.get_accounts().await.unwrap();

    let first_account = accounts.first().expect("account to exist");
    let account = client.get_account(&first_account.address).await.unwrap();

    assert_eq!(&account, first_account);
}

#[async_test]
async fn get_balance() {
    let account = load_account();
    let client = new_grpc_client();

    let coin = client.get_balance(&account.address, "utia").await.unwrap();
    assert_eq!("utia", &coin.denom);
    assert!(coin.amount > 0);

    let all_coins = client.get_all_balances(&account.address).await.unwrap();
    assert!(!all_coins.is_empty());
    assert!(all_coins.iter().map(|c| c.amount).sum::<u64>() > 0);

    let spendable_coins = client
        .get_spendable_balances(&account.address)
        .await
        .unwrap();
    assert!(!spendable_coins.is_empty());
    assert!(spendable_coins.iter().map(|c| c.amount).sum::<u64>() > 0);

    let total_supply = client.get_total_supply().await.unwrap();
    assert!(!total_supply.is_empty());
    assert!(total_supply.iter().map(|c| c.amount).sum::<u64>() > 0);
}

#[async_test]
async fn get_min_gas_price() {
    let client = new_grpc_client();
    let gas_price = client.get_min_gas_price().await.unwrap();
    assert!(gas_price > 0.0);
}

#[async_test]
async fn get_block() {
    let client = new_grpc_client();

    let latest_block = client.get_latest_block().await.unwrap();
    let height = latest_block.header.height.value() as i64;

    let block = client.get_block_by_height(height).await.unwrap();
    assert_eq!(block.header, latest_block.header);
}

#[async_test]
async fn get_blob_params() {
    let client = new_grpc_client();
    let params = client.get_blob_params().await.unwrap();
    assert!(params.gas_per_blob_byte > 0);
    assert!(params.gov_max_square_size > 0);
}

#[async_test]
async fn submit_and_get_tx() {
    let (_lock, tx_client) = new_tx_client().await;

    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "bleb".into(), AppVersion::V3).unwrap()];

    let tx = tx_client
        .submit_blobs(&blobs, TxConfig::default())
        .await
        .unwrap();
    let tx2 = tx_client.get_tx(tx.hash).await.unwrap();

    assert_eq!(tx.hash, tx2.tx_response.txhash);
}

#[async_test]
async fn submit_blobs_parallel() {
    let (_lock, tx_client) = new_tx_client().await;
    let tx_client = Arc::new(tx_client);

    let futs = (0..100)
        .map(|n| {
            let tx_client = tx_client.clone();
            spawn(async move {
                let namespace = Namespace::new_v0(&[1, 2, n]).unwrap();
                let blobs =
                    vec![Blob::new(namespace, format!("bleb{n}").into(), AppVersion::V3).unwrap()];

                let response = tx_client
                    .submit_blobs(&blobs, TxConfig::default())
                    .await
                    .unwrap();

                assert!(response.height.value() > 3)
            })
        })
        .collect::<Vec<_>>();

    for fut in futs {
        fut.await.unwrap();
    }
}

#[async_test]
async fn submit_blobs_insufficient_gas_price_and_limit() {
    let (_lock, tx_client) = new_tx_client().await;

    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "bleb".into(), AppVersion::V3).unwrap()];

    let err = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_gas_limit(10000))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::OutOfGas, _, _)
    ));

    let err = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_gas_price(0.0005))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _, _)
    ));
}

#[async_test]
async fn submit_blobs_gas_price_update() {
    let (_lock, tx_client) = new_tx_client().await;

    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "bleb".into(), AppVersion::V3).unwrap()];

    tx_client.set_gas_price(0.0005);

    // if user also set gas price, no update should happen
    let err = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_gas_price(0.0006))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _, _)
    ));
    assert_eq!(tx_client.gas_price(), 0.0005);

    // with default config, gas price should be updated
    tx_client
        .submit_blobs(&blobs, TxConfig::default())
        .await
        .unwrap();
    assert!(tx_client.gas_price() > 0.0005);
}

#[async_test]
async fn submit_message() {
    let account = load_account();
    let other_account = TestAccount::random();
    let amount = Coin::utia(12345);
    let (_lock, tx_client) = new_tx_client().await;

    let msg = MsgSend {
        from_address: account.address.to_string(),
        to_address: other_account.address.to_string(),
        amount: vec![amount.clone().into()],
    };

    tx_client
        .submit_message(msg, TxConfig::default())
        .await
        .unwrap();

    let coins = tx_client
        .get_all_balances(&other_account.address)
        .await
        .unwrap();

    assert_eq!(coins.len(), 1);
    assert_eq!(amount, coins[0]);
}

#[async_test]
async fn submit_message_insufficient_gas_price_and_limit() {
    let account = load_account();
    let other_account = TestAccount::random();
    let amount = Coin::utia(12345);
    let (_lock, tx_client) = new_tx_client().await;

    let msg = MsgSend {
        from_address: account.address.to_string(),
        to_address: other_account.address.to_string(),
        amount: vec![amount.clone().into()],
    };

    let err = tx_client
        .submit_message(msg.clone(), TxConfig::default().with_gas_limit(10000))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::OutOfGas, _, _)
    ));

    let err = tx_client
        .submit_message(msg, TxConfig::default().with_gas_price(0.0005))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _, _)
    ));
}

#[async_test]
async fn submit_message_gas_price_update() {
    let account = load_account();
    let other_account = TestAccount::random();
    let amount = Coin::utia(12345);
    let (_lock, tx_client) = new_tx_client().await;

    let msg = MsgSend {
        from_address: account.address.to_string(),
        to_address: other_account.address.to_string(),
        amount: vec![amount.clone().into()],
    };

    tx_client.set_gas_price(0.0005);

    // if user also set gas price, no update should happen
    let err = tx_client
        .submit_message(msg.clone(), TxConfig::default().with_gas_price(0.0006))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _, _)
    ));
    assert_eq!(tx_client.gas_price(), 0.0005);

    // with default config, gas price should be updated
    tx_client
        .submit_message(msg, TxConfig::default())
        .await
        .unwrap();
    assert!(tx_client.gas_price() > 0.0005);
}
