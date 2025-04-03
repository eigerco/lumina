use std::env;
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use celestia_rpc::prelude::*;
use celestia_rpc::{Client, TxConfig};
use celestia_types::Blob;
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::ClientError;
use tokio::sync::{Mutex, MutexGuard};

// Use node-2 (light node) as the default RPC URL
const CELESTIA_RPC_URL: &str = "ws://localhost:46658";

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

fn token_from_env(auth_level: AuthLevel) -> Result<Option<String>> {
    match auth_level {
        AuthLevel::Skip => Ok(None),
        AuthLevel::Read => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_READ")?)),
        AuthLevel::Write => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_WRITE")?)),
        AuthLevel::Admin => Ok(Some(env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?)),
    }
}

fn env_or(var_name: &str, or_value: &str) -> String {
    env::var(var_name).unwrap_or_else(|_| or_value.to_owned())
}

pub async fn new_test_client_with_url(
    auth_level: AuthLevel,
    celestia_rpc_url: &str,
) -> Result<Client> {
    let _ = dotenvy::dotenv();
    let token = token_from_env(auth_level)?;
    let url = env_or("CELESTIA_RPC_URL", celestia_rpc_url);

    let client = Client::new(&url, token.as_deref()).await?;

    while client.header_network_head().await?.height().value() < 2 {
        sleep(Duration::from_secs(1));
    }

    Ok(client)
}

pub async fn new_test_client(auth_level: AuthLevel) -> Result<Client> {
    let url = env_or("CELESTIA_RPC_URL", CELESTIA_RPC_URL);
    new_test_client_with_url(auth_level, &url).await
}

pub async fn blob_submit<C>(client: &C, blobs: &[Blob]) -> Result<u64, ClientError>
where
    C: SubscriptionClientT + Sync,
{
    let _guard = write_lock().await;
    client.blob_submit(blobs, TxConfig::default()).await
}
