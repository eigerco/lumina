use std::env;

use anyhow::Result;
use celestia_rpc::client::new_http;
use celestia_rpc::prelude::*;
use celestia_types::nmt::{Namespace, NS_ID_V0_SIZE};
use jsonrpsee::http_client::HttpClient;
use rand::{Rng, RngCore};

pub mod client;

const CONN_STR: &str = "http://localhost:26658";

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

pub async fn test_client(auth_level: AuthLevel) -> Result<HttpClient> {
    let _ = dotenvy::dotenv();
    let token = token_from_env(auth_level)?;

    let client = new_http(CONN_STR, token.as_deref())?;

    // minimum 2 blocks
    client.header_wait_for_height(2).await?;

    Ok(client)
}

fn ns_to_u128(ns: Namespace) -> u128 {
    let mut bytes = [0u8; 16];
    let id = ns.id_v0().unwrap();
    bytes[6..].copy_from_slice(id);
    u128::from_be_bytes(bytes)
}

pub fn random_ns() -> Namespace {
    Namespace::const_v0(random_bytes_array())
}

pub fn random_ns_range(start: Namespace, end: Namespace) -> Namespace {
    let start = ns_to_u128(start);
    let end = ns_to_u128(end);

    let num = rand::thread_rng().gen_range(start..end);
    let bytes = num.to_be_bytes();
    let id = &bytes[bytes.len() - NS_ID_V0_SIZE..];

    Namespace::new_v0(id).unwrap()
}

pub fn random_bytes(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

pub fn random_bytes_array<const N: usize>() -> [u8; N] {
    std::array::from_fn(|_| rand::random())
}
