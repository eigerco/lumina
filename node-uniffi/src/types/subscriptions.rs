use celestia_types::{Blob, ExtendedHeader};
use futures::StreamExt;
use lumina_node::node::SubscriptionError as NodeSubscriptionError;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use uniffi::Object;

use crate::error::LuminaError;

type HeaderSubscriptionItem = Result<ExtendedHeader, NodeSubscriptionError>;
type BlobSubscriptionItem = Result<(u64, Vec<Blob>), NodeSubscriptionError>;

#[derive(Object)]
pub struct HeaderStream {
    stream: RwLock<ReceiverStream<HeaderSubscriptionItem>>,
}

impl HeaderStream {
    pub(crate) fn new(stream: ReceiverStream<HeaderSubscriptionItem>) -> Self {
        HeaderStream {
            stream: RwLock::new(stream),
        }
    }
}

#[uniffi::export]
impl HeaderStream {
    pub async fn next(&self) -> Result<String, SubscriptionError> {
        let header = self
            .stream
            .write()
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
pub struct BlobStream {
    stream: RwLock<ReceiverStream<BlobSubscriptionItem>>,
}

impl BlobStream {
    pub(crate) fn new(stream: ReceiverStream<BlobSubscriptionItem>) -> Self {
        BlobStream {
            stream: RwLock::new(stream),
        }
    }
}

#[uniffi::export]
impl BlobStream {
    pub async fn next(&self) -> Result<BlobAtHeight, SubscriptionError> {
        let (height, blob) = self
            .stream
            .write()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map_err(SubscriptionError::from)?;

        Ok(BlobAtHeight { height, blob })
    }
}

#[derive(uniffi::Object)]
pub struct BlobAtHeight {
    pub height: u64,
    pub blob: Vec<Blob>,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Unable to receive subscription item at {height}: {error}")]
    NodeError { height: u64, error: LuminaError },
    #[error("Subscription stream ended")]
    StreamEnded,
    #[error("Unable to serialize header: {0}")]
    Serialization(String),
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
