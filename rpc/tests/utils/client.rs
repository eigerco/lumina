use anyhow::Result;
use celestia_rpc::client::new_websocket;
use celestia_rpc::prelude::*;
use celestia_types::Blob;
use jsonrpsee::core::Error;
use jsonrpsee::ws_client::WsClient;
use once_cell::sync::Lazy;
use std::env;
use tokio::sync::Mutex;

static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

const CONN_STR: &str = "ws://localhost:26658";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AuthLevel {
    Public,
    Read,
    Write,
    Admin,
}

fn token_from_env(auth_level: AuthLevel) -> Result<Option<String>> {
    match auth_level {
        AuthLevel::Public => Ok(None),
        AuthLevel::Read => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_READ")?)),
        AuthLevel::Write => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_WRITE")?)),
        AuthLevel::Admin => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?)),
    }
}

pub async fn new_test_client(auth_level: AuthLevel) -> Result<WsClient> {
    let _ = dotenvy::dotenv();
    let token = token_from_env(auth_level)?;

    let client = new_websocket(CONN_STR, token.as_deref()).await?;

    // minimum 2 blocks
    client.header_wait_for_height(2).await?;

    Ok(client)
}

pub async fn blob_submit(client: &WsClient, blobs: &[Blob]) -> Result<u64, Error> {
    let _guard = LOCK.lock().await;
    client.blob_submit(&blobs).await
}
