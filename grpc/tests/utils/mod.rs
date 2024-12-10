use std::time::Duration;

use anyhow::Result;
use celestia_grpc::GrpcClient;
use celestia_types::state::Address;
use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
use tendermint::public_key::Secp256k1 as VerifyingKey;

#[cfg(not(target_arch = "wasm32"))]
const CELESTIA_GRPC_URL: &str = "http://localhost:19090";
#[cfg(target_arch = "wasm32")]
const CELESTIA_GRPCWEB_PROXY_URL: &str = "http://localhost:18080";

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

#[cfg(not(target_arch = "wasm32"))]
pub fn new_test_client() -> Result<GrpcClient<tonic::transport::Channel>> {
    let _ = dotenvy::dotenv();
    let url = std::env::var("CELESTIA_GRPC_URL").unwrap_or_else(|_| CELESTIA_GRPC_URL.into());

    Ok(GrpcClient::with_url(url)?)
}

#[cfg(target_arch = "wasm32")]
pub fn new_test_client() -> Result<GrpcClient<tonic_web_wasm_client::Client>> {
    Ok(GrpcClient::with_grpcweb_url(CELESTIA_GRPCWEB_PROXY_URL))
}

pub fn load_account() -> TestAccount {
    let address = include_str!("../../../ci/credentials/bridge-0.addr");
    let hex_key = include_str!("../../../ci/credentials/bridge-0.plaintext-key");

    let signing_key =
        SigningKey::from_slice(&hex::decode(hex_key.trim()).expect("valid hex representation"))
            .expect("valid key material");

    TestAccount {
        address: address.trim().parse().expect("valid address"),
        verifying_key: *signing_key.verifying_key(),
        signing_key,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: Duration) {
    let millis = u32::try_from(duration.as_millis().max(1)).unwrap_or(u32::MAX);
    let delay = gloo_timers::future::TimeoutFuture::new(millis);
    delay.await;
}
