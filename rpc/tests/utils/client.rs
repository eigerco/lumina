use std::env;
use std::sync::OnceLock;

use anyhow::Result;
use celestia_rpc::prelude::*;
use celestia_rpc::Client;
use celestia_types::{blob::SubmitOptions, Blob};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::Error;
use tokio::sync::{Mutex, MutexGuard};

const CELESTIA_RPC_URL: &str = "ws://localhost:26658";

async fn write_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

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

fn env_or(var_name: &str, or_value: &str) -> String {
    env::var(var_name).unwrap_or_else(|_| or_value.to_owned())
}

pub async fn new_test_client(auth_level: AuthLevel) -> Result<Client> {
    let _ = dotenvy::dotenv();
    let token = token_from_env(auth_level)?;
    let url = env_or("CELESTIA_RPC_URL", CELESTIA_RPC_URL);

    let client = Client::new(&url, token.as_deref()).await?;

    // minimum 2 blocks
    client.header_wait_for_height(2).await?;

    Ok(client)
}

pub async fn blob_submit<C>(client: &C, blobs: &[Blob]) -> Result<u64, Error>
where
    C: ClientT + Sync,
{
    let _guard = write_lock().await;
    client.blob_submit(blobs, SubmitOptions::default()).await
}
