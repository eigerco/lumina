use std::sync::OnceLock;

use celestia_types::state::{AccAddress, ValAddress};
use k256::ecdsa::SigningKey;
use tokio::sync::{Mutex, MutexGuard};

use crate::tx::TxConfig;
use crate::Client;

const TEST_PRIV_KEY: &str = include_str!("../../ci/credentials/node-0.plaintext-key");
const TEST_RPC_URL: &str = "ws://localhost:26658";

#[cfg(not(target_arch = "wasm32"))]
const TEST_GRPC_URL: &str = "http://localhost:19090";

/// gRPC-Web url
#[cfg(target_arch = "wasm32")]
const TEST_GRPC_URL: &str = "http://localhost:18080";

// We have to sequence the tests which submits transactions.
// Multiple independent tx clients don't work well in parallel
// as they break each other's account.sequence
pub(crate) async fn new_client() -> (MutexGuard<'static, ()>, Client) {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(())).lock().await;

    let client = Client::builder()
        .rpc_url(TEST_RPC_URL)
        .grpc_url(TEST_GRPC_URL)
        .private_key_hex(TEST_PRIV_KEY)
        .build()
        .await
        .unwrap();

    (lock, client)
}

pub(crate) async fn new_read_only_client() -> Client {
    Client::builder()
        .rpc_url(TEST_RPC_URL)
        .build()
        .await
        .unwrap()
}

// This needs to be called *before* `new_client`, otherwise it will deadlock.
pub(crate) async fn new_client_random_account() -> Client {
    let (_lock, client) = new_client().await;

    let random_key = SigningKey::random(&mut rand::thread_rng());
    let random_acc = random_key.verifying_key().into();

    // Fund the account with 2000 utai
    client
        .state()
        .transfer(&random_acc, 2000, TxConfig::default())
        .await
        .unwrap();

    Client::builder()
        .rpc_url(TEST_RPC_URL)
        .grpc_url(TEST_GRPC_URL)
        .keypair(random_key)
        .build()
        .await
        .unwrap()
}

pub(crate) fn validator_address() -> ValAddress {
    let s = include_str!("../../ci/credentials/validator-0.valaddr");
    s.trim().parse().expect("invalid validator address")
}

pub(crate) fn node0_address() -> AccAddress {
    let s = include_str!("../../ci/credentials/node-0.addr");
    s.trim().parse().expect("invalid account address")
}
