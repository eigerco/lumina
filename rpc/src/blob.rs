//! celestia-node rpc types and methods related to blobs

use std::future::Future;
use std::marker::{Send, Sync};
use std::pin::Pin;

use async_stream::try_stream;
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::{Blob, Commitment};
use futures::{Stream, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::client::{ClientT, Error};
use jsonrpsee::proc_macros::rpc;
use serde::{Deserialize, Serialize};

use crate::TxConfig;
#[cfg(target_arch = "wasm32")]
use crate::custom_client_error;

/// Response type for [`BlobClient::blob_subscribe`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlobsAtHeight {
    /// Blobs submitted at given height.
    pub blobs: Option<Vec<Blob>>,
    /// A height for which the blobs were returned.
    pub height: u64,
}

mod rpc {
    use super::*;

    #[rpc(client, namespace = "blob", namespace_separator = ".")]
    pub trait Blob {
        #[method(name = "Get")]
        async fn blob_get(
            &self,
            height: u64,
            namespace: Namespace,
            commitment: Commitment,
        ) -> Result<Blob, Error>;

        #[method(name = "GetAll")]
        async fn blob_get_all(
            &self,
            height: u64,
            namespaces: &[Namespace],
        ) -> Result<Option<Vec<Blob>>, Error>;

        #[method(name = "GetProof")]
        async fn blob_get_proof(
            &self,
            height: u64,
            namespace: Namespace,
            commitment: Commitment,
        ) -> Result<Vec<NamespaceProof>, Error>;

        #[method(name = "Included")]
        async fn blob_included(
            &self,
            height: u64,
            namespace: Namespace,
            proof: &NamespaceProof,
            commitment: Commitment,
        ) -> Result<bool, Error>;

        #[method(name = "Submit")]
        async fn blob_submit(&self, blobs: &[Blob], opts: TxConfig) -> Result<u64, Error>;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[rpc(client, namespace = "blob", namespace_separator = ".")]
    pub trait BlobSubscription {
        /// Subscribe to published blobs from the given namespace as they are included.
        ///
        /// # Notes
        ///
        /// Unsubscribe is not implemented by Celestia nodes.
        #[subscription(name = "Subscribe", unsubscribe = "Unsubscribe", item = BlobsAtHeight)]
        async fn blob_subscribe(&self, namespace: Namespace) -> SubscriptionResult;
    }
}

/// Client implementation for the `Blob` RPC API.
pub trait BlobClient: ClientT {
    /// Get retrieves the blob by commitment under the given namespace and height.
    fn blob_get<'a, 'fut>(
        &'a self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> impl Future<Output = Result<Blob, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobClient::blob_get(self, height, namespace, commitment)
    }

    /// GetAll returns all blobs under the given namespaces and height.
    fn blob_get_all<'a, 'b, 'fut>(
        &'a self,
        height: u64,
        namespaces: &'b [Namespace],
    ) -> impl Future<Output = Result<Option<Vec<Blob>>, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobClient::blob_get_all(self, height, namespaces)
    }

    /// GetProof retrieves proofs in the given namespaces at the given height by commitment.
    fn blob_get_proof<'a, 'fut>(
        &'a self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> impl Future<Output = Result<Vec<NamespaceProof>, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobClient::blob_get_proof(self, height, namespace, commitment)
    }

    /// Included checks whether a blob's given commitment(Merkle subtree root) is included at given height and under the namespace.
    fn blob_included<'a, 'b, 'fut>(
        &'a self,
        height: u64,
        namespace: Namespace,
        proof: &'b NamespaceProof,
        commitment: Commitment,
    ) -> impl Future<Output = Result<bool, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobClient::blob_included(self, height, namespace, proof, commitment)
    }

    /// Submit sends Blobs and reports the height in which they were included. Allows sending multiple Blobs atomically synchronously. Uses default wallet registered on the Node.
    fn blob_submit<'a, 'b, 'fut>(
        &'a self,
        blobs: &'b [Blob],
        opts: TxConfig,
    ) -> impl Future<Output = Result<u64, Error>> + Send + 'fut
    where
        'a: 'fut,
        'b: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::BlobClient::blob_submit(self, blobs, opts)
    }

    /// Subscribe to published blobs from the given namespace as they are included.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[cfg(not(target_arch = "wasm32"))]
    fn blob_subscribe<'a>(
        &'a self,
        namespace: Namespace,
    ) -> Pin<Box<dyn Stream<Item = Result<BlobsAtHeight, Error>> + Send + 'a>>
    where
        Self: SubscriptionClientT + Sized + Sync,
    {
        try_stream! {
            let mut subscription = rpc::BlobSubscriptionClient::blob_subscribe(self, namespace).await?;

            while let Some(blobs_at_height) = subscription.next().await {
                // TODO: Should we validate blobs?
                yield blobs_at_height?;
            }
        }
        .boxed()
    }

    /// Subscribe to published blobs from the given namespace as they are included.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[cfg(target_arch = "wasm32")]
    fn blob_subscribe<'a>(
        &'a self,
        namespace: Namespace,
    ) -> Pin<Box<dyn Stream<Item = Result<BlobsAtHeight, Error>> + Send + 'a>>
    where
        Self: Sized + Sync,
    {
        try_stream! {
            let mut subscription = super::HeaderClient::header_subscribe(self);

            while let Some(header) = subscription.next().await {
                let header = header?;
                let height = header.height().value();
                let blobs = rpc::BlobClient::blob_get_all(self, height, &[namespace]).await?;

                if let Some(blobs) = &blobs {
                    let app_version = header.app_version().map_err(custom_client_error)?;
                    for blob in blobs {
                        blob.validate(app_version).map_err(custom_client_error)?;
                    }
                }

                yield BlobsAtHeight { blobs, height };
            }
        }
        .boxed()
    }
}

impl<T> BlobClient for T where T: ClientT {}
