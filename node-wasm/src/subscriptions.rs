use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use wasm_bindgen::prelude::*;

use celestia_types::{Blob, ExtendedHeader};

use crate::{ports::Port, worker::SubscriptionFeedback};

#[wasm_bindgen(getter_with_clone)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionError {
    pub height: Option<u64>,
    pub error: String,
}

#[wasm_bindgen]
pub struct HeaderStream {
    channel: mpsc::UnboundedReceiver<Result<ExtendedHeader, SubscriptionError>>,
    port: Port,
}

#[wasm_bindgen]
pub struct BlobStream {
    channel: mpsc::UnboundedReceiver<Result<(u64, Blob), SubscriptionError>>,
    port: Port,
}

#[wasm_bindgen(getter_with_clone)]
pub struct BlobAtHeight {
    pub height: u64,
    pub blob: Blob,
}

#[wasm_bindgen]
impl HeaderStream {
    pub(crate) fn new(
        channel: mpsc::UnboundedReceiver<Result<ExtendedHeader, SubscriptionError>>,
        port: Port,
    ) -> Self {
        HeaderStream { channel, port }
    }

    pub async fn next(&mut self) -> Result<Option<ExtendedHeader>, SubscriptionError> {
        self.port
            .send_raw(&SubscriptionFeedback::Ready)
            .map_err(|e| SubscriptionError {
                height: None,
                error: format!("error sending permit: {e}"),
            })?;
        self.channel.recv().await.transpose()
    }
}

#[wasm_bindgen]
impl BlobStream {
    pub(crate) fn new(
        channel: mpsc::UnboundedReceiver<Result<(u64, Blob), SubscriptionError>>,
        port: Port,
    ) -> Self {
        BlobStream { channel, port }
    }

    pub async fn next(&mut self) -> Result<Option<BlobAtHeight>, SubscriptionError> {
        self.port
            .send_raw(&SubscriptionFeedback::Ready)
            .map_err(|e| SubscriptionError {
                height: None,
                error: format!("error sending permit: {e}"),
            })?;
        Ok(self
            .channel
            .recv()
            .await
            .transpose()?
            .map(|(height, blob)| BlobAtHeight { height, blob }))
    }
}
