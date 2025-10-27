use std::sync::Arc;

use celestia_types::{Blob, ExtendedHeader};
use futures::StreamExt;
use lumina_node::node::SubscriptionError as NodeSubscriptionError;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use uniffi::Object;

use crate::error::LuminaError;

type HeaderSubscriptionItem = Result<ExtendedHeader, NodeSubscriptionError>;
type BlobSubscriptionItem = Result<(u64, Vec<Blob>), NodeSubscriptionError>;

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Unable to receive subscription item at {height}: {error}")]
    NodeError { height: u64, error: LuminaError },
    #[error("Subscription stream ended")]
    StreamEnded,
    #[error("Unable to serialize header: {0}")]
    Serialization(String),
}

#[derive(Object)]
pub struct HeaderStream(Mutex<ReceiverStream<HeaderSubscriptionItem>>);

impl HeaderStream {
    pub(crate) fn new(stream: ReceiverStream<HeaderSubscriptionItem>) -> Self {
        HeaderStream(Mutex::new(stream))
    }
}

#[uniffi::export]
impl HeaderStream {
    pub async fn next(&self) -> Result<String, SubscriptionError> {
        let header = self
            .0
            .lock()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map_err(SubscriptionError::from)?;
        // TODO: replace with plain ExtendedHeader, once it's uniffied
        Ok(serde_json::to_string(&header)?)
    }
}

#[derive(Object)]
pub struct BlobStream(Mutex<ReceiverStream<BlobSubscriptionItem>>);

impl BlobStream {
    pub(crate) fn new(stream: ReceiverStream<BlobSubscriptionItem>) -> Self {
        BlobStream(Mutex::new(stream))
    }
}

#[uniffi::export]
impl BlobStream {
    pub async fn next(&self) -> Result<BlobsAtHeight, SubscriptionError> {
        let (height, blobs) = self
            .0
            .lock()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map_err(SubscriptionError::from)?;

        let blobs = blobs.into_iter().map(Arc::new).collect();

        Ok(BlobsAtHeight { height, blobs })
    }
}

#[derive(Debug, Clone, uniffi::Object)]
pub struct BlobsAtHeight {
    pub height: u64,
    pub blobs: Vec<Arc<Blob>>,
}

#[uniffi::export]
impl BlobsAtHeight {
    fn height(&self) -> u64 {
        self.height
    }

    fn blobs(self: Arc<Self>) -> Vec<Arc<Blob>> {
        self.blobs.iter().map(Arc::clone).collect()
    }
}

impl From<NodeSubscriptionError> for SubscriptionError {
    fn from(error: NodeSubscriptionError) -> Self {
        SubscriptionError::NodeError {
            height: error.height,
            error: error.source.into(),
        }
    }
}

impl From<serde_json::Error> for SubscriptionError {
    fn from(error: serde_json::Error) -> Self {
        SubscriptionError::Serialization(error.to_string())
    }
}
