use std::future::IntoFuture;
use std::sync::Arc;

use celestia_grpc::{Error, TxConfig};
use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_rpc::HeaderClient;
use celestia_types::nmt::Namespace;
use celestia_types::state::{Coin, ErrorCode};
use celestia_types::{AppVersion, Blob};
use futures::FutureExt;
use lumina_utils::test_utils::async_test;
use utils::{TestAccount, load_account, new_rpc_client};

pub mod utils;

use crate::utils::{new_grpc_client, new_tx_client, spawn};

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
    assert_eq!("utia", coin.denom());
    assert!(coin.amount() > 0);

    let all_coins = client.get_all_balances(&account.address).await.unwrap();
    assert!(!all_coins.is_empty());
    assert!(all_coins.iter().map(|c| c.amount()).sum::<u64>() > 0);

    let spendable_coins = client
        .get_spendable_balances(&account.address)
        .await
        .unwrap();
    assert!(!spendable_coins.is_empty());
    assert!(spendable_coins.iter().map(|c| c.amount()).sum::<u64>() > 0);

    let total_supply = client.get_total_supply().await.unwrap();
    assert!(!total_supply.is_empty());
    assert!(total_supply.iter().map(|c| c.amount()).sum::<u64>() > 0);
}

#[async_test]
async fn get_verified_balance() {
    let client = new_grpc_client();
    let account = load_account();

    let jrpc_client = new_rpc_client().await;

    let (head, expected_balance) = tokio::join!(
        jrpc_client.header_network_head().map(Result::unwrap),
        client
            .get_balance(&account.address, "utia")
            .into_future()
            .map(Result::unwrap)
    );

    // trustless balance queries represent state at header.height - 1, so
    // we need to wait for a new head to compare it with the expected balance
    let head = jrpc_client
        .header_wait_for_height(head.height().value() + 1)
        .await
        .unwrap();

    let verified_balance = client
        .get_verified_balance(&account.address, &head)
        .await
        .unwrap();

    assert_eq!(expected_balance, verified_balance);
}

#[async_test]
async fn get_verified_balance_not_funded_account() {
    let client = new_grpc_client();
    let account = TestAccount::random();

    let jrpc_client = new_rpc_client().await;
    let head = jrpc_client.header_network_head().await.unwrap();

    let verified_balance = client
        .get_verified_balance(&account.address, &head)
        .await
        .unwrap();

    assert_eq!(Coin::utia(0), verified_balance);
}

#[async_test]
async fn get_node_config() {
    let client = new_grpc_client();
    let config = client.get_node_config().await.unwrap();

    // we don't set any explicit value for it in the config
    // so it should be empty since v6
    assert!(config.minimum_gas_price.is_none());
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
async fn query_state_at_block_height_with_metadata() {
    let (_lock, tx_client) = new_tx_client().await;

    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "bleb".into(), None, AppVersion::V3).unwrap()];

    let tx = tx_client
        .submit_blobs(&blobs, TxConfig::default())
        .await
        .unwrap();

    let addr = tx_client.get_account_address().unwrap().into();
    let new_balance = tx_client.get_balance(&addr, "utia").await.unwrap();
    let old_balance = tx_client
        .get_balance(&addr, "utia")
        .block_height(tx.height.value() - 1)
        .await
        .unwrap();

    assert!(new_balance.amount() < old_balance.amount());
}

#[async_test]
async fn submit_and_get_tx() {
    let (_lock, tx_client) = new_tx_client().await;

    let namespace = Namespace::new_v0(&[1, 2, 3]).unwrap();
    let blobs = vec![Blob::new(namespace, "bleb".into(), None, AppVersion::V3).unwrap()];

    let tx = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_memo("foo"))
        .await
        .unwrap();
    let tx2 = tx_client.get_tx(tx.hash).await.unwrap();

    assert_eq!(tx.hash, tx2.tx_response.txhash);
    assert_eq!(tx2.tx.body.memo, "foo");
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
                let blobs = vec![
                    Blob::new(namespace, format!("bleb{n}").into(), None, AppVersion::V3).unwrap(),
                ];

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
    let blobs = vec![Blob::new(namespace, "bleb".into(), None, AppVersion::V3).unwrap()];

    let err = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_gas_limit(10000))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::OutOfGas, _)
    ));

    let err = tx_client
        .submit_blobs(&blobs, TxConfig::default().with_gas_price(0.0005))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _)
    ));
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
        Error::TxBroadcastFailed(_, ErrorCode::OutOfGas, _)
    ));

    let err = tx_client
        .submit_message(msg, TxConfig::default().with_gas_price(0.0005))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _)
    ));
}

#[async_test]
async fn tx_client_is_send_and_sync() {
    fn is_send_and_sync<T: Send + Sync>(_: &T) {}
    fn is_send<T: Send>(_: &T) {}

    let (_lock, tx_client) = new_tx_client().await;
    is_send_and_sync(&tx_client);

    is_send(
        &tx_client
            .submit_blobs(&[], TxConfig::default())
            .into_future(),
    );
    is_send(
        &tx_client
            .submit_message(
                MsgSend {
                    from_address: "".into(),
                    to_address: "".into(),
                    amount: vec![],
                },
                TxConfig::default(),
            )
            .into_future(),
    );
}
