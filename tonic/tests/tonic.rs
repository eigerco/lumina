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
    let height = latest_block.0.header.height.value() as i64;

    let block = client.get_block_by_height(height).await.unwrap();
    assert_eq!(block.0.header, latest_block.0.header);
}

#[tokio::test]
async fn get_account() {
    let mut client = new_test_client().await.unwrap();

    let accounts = client.get_accounts().await.unwrap();

    println!("first account : {:?}", accounts.first());

    let acct = client
        .get_account("celestia1p3ucd3ptpw902fluyjzhq3ffgq4ntddaf0pdta".to_string())
        .await
        .unwrap();

    println!("{acct:?}");
}
