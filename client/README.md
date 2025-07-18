A high-level client for interacting with a Celestia node.

It combines the functionality of [`celestia-rpc`] and [`celestia-grpc`] crates.

There are two modes: read-only mode and submit mode. Read-only mode requires
RPC endpoint, while submit mode requires RPC and gRPC endpoints plus a signer.

# Examples

Read-only mode:

```rust,no_run
use celestia_client::{Client, Result};

const RPC_URL: &str = "ws://localhost:26658";

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::builder()
        .rpc_url(RPC_URL)
        .build()
        .await?;

    let header = client.header().head().await?;
    dbg!(header);

    Ok(())
}
```

Submit mode:

```rust,no_run
use std::env;

use celestia_client::{Client, Result};
use celestia_client::tx::TxConfig;

const RPC_URL: &str = "ws://localhost:26658";
const GRPC_URL : &str = "http://localhost:19090";

#[tokio::main]
async fn main() -> Result<()> {
    let priv_key = env::var("CELESTIA_PRIV_KEY")
        .expect("Environment variable CELESTIA_PRIV_KEY not set");

    let client = Client::builder()
        .rpc_url(RPC_URL)
        .grpc_url(GRPC_URL)
        .private_key_hex(&priv_key)
        .build()
        .await?;

    let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
    let tx_info = client.state().transfer(&to_address, 12345, TxConfig::default()).await?;
    dbg!(tx_info);

    Ok(())
}
```

Submitting and retrieving a blob:

```rust,no_run
use std::env;

use celestia_client::{Client, Result};
use celestia_client::tx::TxConfig;
use celestia_types::nmt::Namespace;
use celestia_types::{AppVersion, Blob};

const RPC_URL: &str = "ws://localhost:26658";
const GRPC_URL: &str = "http://localhost:19090";

#[tokio::main]
async fn main() -> Result<()> {
    let priv_key = env::var("CELESTIA_PRIV_KEY")
        .expect("Environment variable CELESTIA_PRIV_KEY not set");

    let client = Client::builder()
        .rpc_url(RPC_URL)
        .grpc_url(GRPC_URL)
        .private_key_hex(&priv_key)
        .build()
        .await?;

    // Create the blob
    let ns = Namespace::new_v0(b"mydata")?;
    let blob = Blob::new_with_signer(
        ns,
        b"some data to store".to_vec(),
        client.state().account_address()?,
        AppVersion::V3,
    )?;

    // This is the hash of the blob and is needed for retrieving blob form chain.
    let commitment = blob.commitment.clone();

    // Submit the blob
    let tx_info = client.blob().submit(&[blob], TxConfig::default()).await?;

    // Retrieve the blob. Blob is validated within the `get` method, so
    // we don't need to do anything else.
    let received_blob = client
        .blob()
        .get(tx_info.height.value(), ns, commitment)
        .await?;

    println!("Data: {:?}", str::from_utf8(&received_blob.data).unwrap());

    Ok(())
}
```

[`celestia-rpc`]: celestia_rpc
[`celestia-grpc`]: celestia_grpc
