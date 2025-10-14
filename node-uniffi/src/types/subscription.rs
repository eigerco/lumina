use celestia_types::{Blob, ExtendedHeader};
use futures::StreamExt;
use lumina_node::node::SubscriptionError;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use uniffi::Object;

use crate::error::LuminaError;

#[derive(Object)]
pub struct HeaderStream {
    stream: RwLock<ReceiverStream<Result<ExtendedHeader, SubscriptionError>>>,
}

impl HeaderStream {
    pub(crate) fn new(stream: ReceiverStream<Result<ExtendedHeader, SubscriptionError>>) -> Self {
        HeaderStream {
            stream: RwLock::new(stream),
        }
    }
}

#[uniffi::export]
impl HeaderStream {
    pub async fn next(&self) -> Result<String, Error> {
        let header = self
            .stream
            .write()
            .await
            .next()
            .await
            .ok_or(Error::StreamEnded)?
            .map_err(Error::from)?;
        // TODO: replace with plain ExtendedHeader, once it's uniffied
        Ok(serde_json::to_string(&header)?)
    }
}

#[derive(Object)]
pub struct BlobStream {
    stream: RwLock<ReceiverStream<Result<(u64, Vec<Blob>), SubscriptionError>>>,
}

impl BlobStream {
    pub(crate) fn new(stream: ReceiverStream<Result<(u64, Vec<Blob>), SubscriptionError>>) -> Self {
        BlobStream {
            stream: RwLock::new(stream),
        }
    }
}

#[uniffi::export]
impl BlobStream {
    pub async fn next(&self) -> Result<BlobAtHeight, Error> {
        let (height, blob) = self
            .stream
            .write()
            .await
            .next()
            .await
            .ok_or(Error::StreamEnded)?
            .map_err(Error::from)?;

        Ok(BlobAtHeight { height, blob })
    }
}

#[derive(uniffi::Object)]
pub struct BlobAtHeight {
    pub height: u64,
    pub blob: Vec<Blob>,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum Error {
    #[error("Unable to receive subscription item at {height}: {error}")]
    Subscription { height: u64, error: LuminaError },
    #[error("Subscription stream ended")]
    StreamEnded,
    #[error("Unable to serialize header: {0}")]
    Serialization(String),
}

impl From<SubscriptionError> for Error {
    fn from(error: SubscriptionError) -> Self {
        Error::Subscription {
            height: error.height,
            error: error.source.into(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Serialization(error.to_string())
    }
}
