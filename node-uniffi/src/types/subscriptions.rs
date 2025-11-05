use std::sync::Arc;

use celestia_types::blob::{Blob, BlobsAtHeight as RustBlobsAtHeight};
use celestia_types::{ExtendedHeader, Share, SharesAtHeight as RustSharesAtHeight};
use futures::StreamExt;
use lumina_node::node::subscriptions::SubscriptionError as NodeSubscriptionError;
use tokio::sync::Mutex;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use uniffi::Object;

use crate::error::LuminaError;

type BlobsSubscriptionItem = Result<RustBlobsAtHeight, NodeSubscriptionError>;
type SharesSubscriptionItem = Result<RustSharesAtHeight, NodeSubscriptionError>;

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Error retreiving subscription item
    #[error("Unable to receive subscription item at {height}: {error}")]
    NodeError { height: u64, error: LuminaError },
    /// Receiver lagged too far behind and the subscription will restart from the current head
    #[error("Subscription item height already pruned from the store, skipping {0} items")]
    Lagged(u64),
    /// Unexpected node subscription stream close
    #[error("Subscription stream ended")]
    StreamEnded,
    /// Error serializing header
    #[error("Unable to serialize header: {0}")]
    Serialization(String),
}

#[derive(Object)]
pub struct HeaderStream(Mutex<BroadcastStream<ExtendedHeader>>);

impl HeaderStream {
    pub(crate) fn new(stream: BroadcastStream<ExtendedHeader>) -> Self {
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
            .map_err(NodeSubscriptionError::from)?;

        // TODO: replace with plain ExtendedHeader, once it's uniffied
        Ok(serde_json::to_string(&header)?)
    }
}

#[derive(Object)]
pub struct BlobsStream(Mutex<ReceiverStream<BlobsSubscriptionItem>>);

impl BlobsStream {
    pub(crate) fn new(stream: ReceiverStream<BlobsSubscriptionItem>) -> Self {
        BlobsStream(Mutex::new(stream))
    }
}

#[uniffi::export]
impl BlobsStream {
    pub async fn next(&self) -> Result<BlobsAtHeight, SubscriptionError> {
        self.0
            .lock()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map(BlobsAtHeight::from)
            .map_err(SubscriptionError::from)
    }
}

#[derive(Debug, Clone, Object)]
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

impl From<RustBlobsAtHeight> for BlobsAtHeight {
    fn from(value: RustBlobsAtHeight) -> Self {
        BlobsAtHeight {
            height: value.height,
            blobs: value.blobs.into_iter().map(Arc::new).collect(),
        }
    }
}

#[derive(Object)]
pub struct SharesStream(Mutex<ReceiverStream<SharesSubscriptionItem>>);

impl SharesStream {
    pub(crate) fn new(stream: ReceiverStream<SharesSubscriptionItem>) -> SharesStream {
        SharesStream(Mutex::new(stream))
    }
}

#[uniffi::export]
impl SharesStream {
    pub async fn next(&self) -> Result<SharesAtHeight, SubscriptionError> {
        self.0
            .lock()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map(SharesAtHeight::from)
            .map_err(SubscriptionError::from)
    }
}

#[derive(Debug, Clone, Object)]
pub struct SharesAtHeight {
    pub height: u64,
    pub shares: Vec<Arc<Share>>,
}

#[uniffi::export]
impl SharesAtHeight {
    fn height(&self) -> u64 {
        self.height
    }

    fn shares(self: Arc<Self>) -> Vec<Arc<Share>> {
        self.shares.iter().map(Arc::clone).collect()
    }
}

impl From<RustSharesAtHeight> for SharesAtHeight {
    fn from(value: RustSharesAtHeight) -> Self {
        SharesAtHeight {
            height: value.height,
            shares: value.shares.into_iter().map(Arc::new).collect(),
        }
    }
}

impl From<NodeSubscriptionError> for SubscriptionError {
    fn from(error: NodeSubscriptionError) -> Self {
        match error {
            NodeSubscriptionError::Node { height, source } => SubscriptionError::NodeError {
                height,
                error: source.into(),
            },
            NodeSubscriptionError::Lagged(skipped) => SubscriptionError::Lagged(skipped),
        }
    }
}

impl From<serde_json::Error> for SubscriptionError {
    fn from(error: serde_json::Error) -> Self {
        SubscriptionError::Serialization(error.to_string())
    }
}
