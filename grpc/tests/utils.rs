#![cfg(not(target_arch = "wasm32"))]

use std::env;

use anyhow::Result;
use celestia_tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
use celestia_types::auth::AccountKeypair;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

use celestia_grpc::GrpcClient;

const CELESTIA_GRPC_URL: &str = "http://localhost:19090";
//const CELESTIA_GRPC_URL: &str = "https://rpc.celestia.pops.one:9090";

#[derive(Clone)]
pub struct TestAuthInterceptor {
    token: Option<MetadataValue<Ascii>>,
}

impl Interceptor for TestAuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            request
                .metadata_mut()
                .insert("authorization", token.clone());
        }
        Ok(request)
    }
}

impl TestAuthInterceptor {
    pub fn new(bearer_token: Option<String>) -> Result<TestAuthInterceptor> {
        let token = bearer_token.map(|token| token.parse()).transpose()?;
        Ok(Self { token })
    }
}

pub fn env_or(var_name: &str, or_value: &str) -> String {
    env::var(var_name).unwrap_or_else(|_| or_value.to_owned())
}

pub async fn new_test_client() -> Result<GrpcClient<TestAuthInterceptor>> {
    let _ = dotenvy::dotenv();
    let url = env_or("CELESTIA_GRPC_URL", CELESTIA_GRPC_URL);
    let grpc_channel = Channel::from_shared(url)?.connect().await?;

    let auth_interceptor = TestAuthInterceptor::new(None)?;
    Ok(GrpcClient::new(grpc_channel, auth_interceptor))
}

pub fn load_account_key(key_bytes: &[u8]) -> AccountKeypair {
    let signing_key = SigningKey::from_slice(key_bytes).unwrap();

    AccountKeypair {
        verifying_key: *signing_key.verifying_key(),
        signing_key,
    }
}
