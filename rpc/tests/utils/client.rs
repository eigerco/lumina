use celestia_rpc::prelude::*;
use celestia_types::Blob;
use jsonrpsee::core::Error;
use jsonrpsee::http_client::HttpClient;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub async fn blob_submit(client: &HttpClient, blob: &Blob) -> Result<u64, Error> {
    let _guard = LOCK.lock().await;
    client.blob_submit(&[blob.to_owned()]).await
}
