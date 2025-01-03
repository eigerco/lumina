use std::sync::Arc;

use celestia_grpc::{Error, TxClient, TxConfig};
use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_types::nmt::Namespace;
use celestia_types::state::{Coin, ErrorCode};
use celestia_types::{AppVersion, Blob};
use k256::ecdsa::VerifyingKey;
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
    // let (_lock, tx_client) = new_tx_client().await;
    let leap_account = TestAccount::from_pk(
        // &hex::decode("4f8fae3480edef0a19d4d5228b09fb37f3dddb9813d6641efee4b1ef95b925c9").unwrap(),
        &hex::decode("fdc8ac75dfa1c142dbcba77938a14dd03078052ce0b49a529dcf72a9885a3abb").unwrap(),
    );
    let grpc_client = new_grpc_client();
    let tx_client = TxClient::new(
        grpc_client,
        leap_account.signing_key,
        &leap_account.address,
        Some(leap_account.verifying_key),
    )
    .await
    .unwrap();

    let publickey = [
        4, 8, 188, 120, 164, 120, 7, 175, 232, 84, 167, 48, 238, 98, 212, 81, 87, 204, 10, 27, 222,
        163, 234, 251, 14, 127, 77, 29, 70, 61, 7, 10, 229, 18, 1, 26, 153, 150, 19, 126, 23, 251,
        235, 180, 50, 39, 188, 212, 225, 126, 199, 54, 190, 234, 155, 217, 131, 23, 152, 212, 7,
        83, 18, 83, 133,
    ];
    let pubkey = VerifyingKey::try_from(&publickey[..]).unwrap();
    assert_eq!(pubkey, leap_account.verifying_key);
    // [170, 9, 246, 238, 124, 93, 176, 134, 9, 150, 251, 49, 57, 168, 239, 85, 1, 123, 213, 219, 203, 75, 241, 125, 240, 52, 69, 80, 69, 2, 102, 254, 21, 59, 193, 232, 205, 142, 226, 126, 105, 157, 197, 162, 161, 85, 77, 183, 232, 10, 185, 195, 230, 51, 46, 168, 10, 50, 48, 147, 223, 140, 174, 38]
    // 0: 86
    let sig = [
        114, 117, 141, 12, 73, 134, 3, 177, 51, 21, 177, 141, 197, 95, 203, 131, 50, 35, 240, 144,
        114, 86, 121, 120, 73, 1, 171, 246, 133, 238, 12, 51, 112, 3, 63, 229, 197, 144, 177, 109,
        44, 158, 234, 172, 158, 105, 9, 217, 141, 214, 166, 151, 151, 17, 109, 85, 174, 250, 233,
        152, 224, 217, 211,
    ];
    let msg = [
        10, 159, 1, 10, 156, 1, 10, 32, 47, 99, 101, 108, 101, 115, 116, 105, 97, 46, 98, 108, 111,
        98, 46, 118, 49, 46, 77, 115, 103, 80, 97, 121, 70, 111, 114, 66, 108, 111, 98, 115, 18,
        120, 10, 47, 99, 101, 108, 101, 115, 116, 105, 97, 49, 54, 57, 115, 53, 48, 112, 115, 121,
        106, 50, 102, 52, 108, 97, 57, 97, 50, 50, 51, 53, 51, 50, 57, 120, 122, 55, 114, 107, 54,
        99, 53, 51, 122, 104, 119, 57, 109, 109, 18, 29, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 98, 99, 26, 1, 4, 34, 32, 49, 100, 4, 151, 53, 254,
        18, 25, 15, 69, 28, 38, 51, 19, 64, 49, 245, 182, 33, 160, 193, 70, 179, 244, 130, 127,
        151, 189, 210, 60, 16, 116, 66, 1, 0, 18, 150, 1, 10, 80, 10, 70, 10, 31, 47, 99, 111, 115,
        109, 111, 115, 46, 99, 114, 121, 112, 116, 111, 46, 115, 101, 99, 112, 50, 53, 54, 107, 49,
        46, 80, 117, 98, 75, 101, 121, 18, 35, 10, 33, 3, 8, 188, 120, 164, 120, 7, 175, 232, 84,
        167, 48, 238, 98, 212, 81, 87, 204, 10, 27, 222, 163, 234, 251, 14, 127, 77, 29, 70, 61, 7,
        10, 229, 18, 4, 10, 2, 8, 1, 24, 1, 18, 66, 10, 11, 10, 4, 117, 116, 105, 97, 18, 3, 49,
        55, 54, 16, 223, 173, 5, 26, 47, 99, 101, 108, 101, 115, 116, 105, 97, 49, 54, 57, 115, 53,
        48, 112, 115, 121, 106, 50, 102, 52, 108, 97, 57, 97, 50, 50, 51, 53, 51, 50, 57, 120, 122,
        55, 114, 107, 54, 99, 53, 51, 122, 104, 119, 57, 109, 109, 26, 7, 112, 114, 105, 118, 97,
        116, 101, 32, 8,
    ];

    let ns = Namespace::new_v0(b"abc").unwrap();
    let blob = Blob::new(ns, b"data".into(), AppVersion::V3).unwrap();
    // let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    // let blobs = vec![Blob::new(namespace, "bleb".into(), AppVersion::V3).unwrap()];

    let tx = tx_client
        .submit_blobs(&[blob], TxConfig::default())
        .await
        .unwrap();
    panic!();
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
        to_address: "celestia159edu39c3mmsudhg63dh4gph8ytpfdpff8q6ew".to_string(), // other_account.address.to_string(),
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
