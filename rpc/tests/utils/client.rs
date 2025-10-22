use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use celestia_rpc::prelude::*;
use celestia_rpc::{Client, TxConfig};
use celestia_types::Blob;
use jsonrpsee::core::ClientError;
use jsonrpsee::core::client::ClientT;
use tokio::sync::{Mutex, MutexGuard};

// Use node-2 (light node) as the default RPC URL
#[cfg(not(target_arch = "wasm32"))]
const CELESTIA_RPC_URL: &str = "ws://localhost:46658";
#[cfg(target_arch = "wasm32")]
const CELESTIA_RPC_URL: &str = "http://localhost:46658";

async fn write_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AuthLevel {
    Skip,
    Read,
    Write,
    Admin,
}

#[cfg(not(target_arch = "wasm32"))]
fn token_from_env(auth_level: AuthLevel) -> Result<Option<String>> {
    match auth_level {
        AuthLevel::Skip => Ok(None),
        AuthLevel::Read => Ok(Some(std::env::var("CELESTIA_NODE_AUTH_TOKEN_READ")?)),
        AuthLevel::Write => Ok(Some(std::env::var("CELESTIA_NODE_AUTH_TOKEN_WRITE")?)),
        AuthLevel::Admin => Ok(Some(std::env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?)),
    }
}

#[cfg(target_arch = "wasm32")]
fn token_from_env(auth_level: AuthLevel) -> Result<Option<String>> {
    let env = include_str!("../../../.env");

    let token_pattern = match auth_level {
        AuthLevel::Skip => return Ok(None),
        AuthLevel::Read => "CELESTIA_NODE_AUTH_TOKEN_READ=",
        AuthLevel::Write => "CELESTIA_NODE_AUTH_TOKEN_WRITE=",
        AuthLevel::Admin => "CELESTIA_NODE_AUTH_TOKEN_ADMIN=",
    };

    env.lines()
        .find_map(|line| line.split_once(token_pattern))
        .map(|(_, token)| token.to_owned())
        .ok_or(anyhow::anyhow!(
            "CELESTIA_NODE_AUTH_TOKEN_<LEVEL> variable must be set during build"
        ))
        .map(Some)
}

#[cfg(not(target_arch = "wasm32"))]
fn env_or(var_name: &str, or_value: &str) -> String {
    std::env::var(var_name).unwrap_or_else(|_| or_value.to_owned())
}

#[cfg(target_arch = "wasm32")]
fn env_or(_var_name: &str, or_value: &str) -> String {
    or_value.to_owned()
}

pub async fn new_test_client_with_url(
    auth_level: AuthLevel,
    celestia_rpc_url: &str,
) -> Result<Client> {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = dotenvy::dotenv();
    let token = token_from_env(auth_level)?;
    let url = env_or("CELESTIA_RPC_URL", celestia_rpc_url);

    let client = Client::new(&url, token.as_deref()).await?;

    while client.header_network_head().await?.height().value() < 2 {
        lumina_utils::time::sleep(Duration::from_secs(1)).await;
    }

    Ok(client)
}

pub async fn new_test_client(auth_level: AuthLevel) -> Result<Client> {
    new_test_client_with_url(auth_level, CELESTIA_RPC_URL).await
}

pub async fn blob_submit_with_config<C>(
    client: &C,
    blobs: &[Blob],
    config: TxConfig,
) -> Result<u64, ClientError>
where
    C: ClientT + Sync,
{
    let _guard = write_lock().await;
    client.blob_submit(blobs, config).await
}

pub async fn blob_submit<C>(client: &C, blobs: &[Blob]) -> Result<u64, ClientError>
where
    C: ClientT + Sync,
{
    blob_submit_with_config(client, blobs, TxConfig::default()).await
}
