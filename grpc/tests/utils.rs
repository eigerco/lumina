#![cfg(not(target_arch = "wasm32"))]

use std::{env, fs};

use anyhow::Result;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

use celestia_grpc::GrpcClient;
use celestia_tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
use celestia_tendermint::public_key::Secp256k1 as VerifyingKey;
use celestia_types::state::Address;

const CELESTIA_GRPC_URL: &str = "http://localhost:19090";

/// [`TestAccount`] stores celestia account credentials and information, for cases where we don't
/// mind jusk keeping the plaintext secret key in memory
#[derive(Debug, Clone)]
pub struct TestAccount {
    /// Bech32 `AccountId` of this account
    pub address: Address,
    /// public key
    pub verifying_key: VerifyingKey,
    /// private key
    pub signing_key: SigningKey,
}

//
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

pub fn load_account(path: &str) -> TestAccount {
    let account_file = format!("{path}.addr");
    let key_file = format!("{path}.plaintext-key");

    let account = fs::read_to_string(account_file).expect("file with account name to exists");
    let hex_encoded_key = fs::read_to_string(key_file).expect("file with plaintext key to exists");

    let signing_key = SigningKey::from_slice(
        &hex::decode(hex_encoded_key.trim()).expect("valid hex representation"),
    )
    .expect("valid key material");

    TestAccount {
        address: account.trim().parse().expect("valid address"),
        verifying_key: *signing_key.verifying_key(),
        signing_key,
    }
}
