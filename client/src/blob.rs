use std::sync::Arc;

use celestia_grpc::DocSigner;
use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::BlobClient;
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::{Blob, Commitment};
use jsonrpsee_core::client::Subscription;

use crate::{Context, Result, TxConfig, TxInfo};

pub struct BlobApi {
    ctx: Arc<Context>,
}

impl BlobApi {
    pub(crate) fn new(ctx: Arc<Context>) -> BlobApi {
        BlobApi { ctx }
    }

    /// Submit given blobs to celestia network.
    ///
    /// When no gas price is specified through config, it will automatically handle updating client’s gas price when consensus updates minimal gas price.
    ///
    /// # Notes
    ///
    /// This is the same as [`StateApi::submit_pay_for_blob`].
    ///
    /// [`StateApi::submit_pay_for_blob`]: crate::StateApi::submit_pay_for_blob
    pub async fn submit(&self, blobs: &[Blob], cfg: TxConfig) -> Result<TxInfo> {
        Ok(self.ctx.grpc()?.submit_blobs(blobs, cfg).await?)
    }

    /// Retrieves the blob by commitment under the given namespace and height.
    pub async fn get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Blob> {
        Ok(self.ctx.rpc.blob_get(height, namespace, commitment).await?)
    }

    /// Retrieves all blobs under the given namespaces and height.
    pub async fn get_all(
        &self,
        height: u64,
        namespaces: &[Namespace],
    ) -> Result<Option<Vec<Blob>>> {
        Ok(self.ctx.rpc.blob_get_all(height, namespaces).await?)
    }

    /// Retrieves proofs in the given namespaces at the given height by commitment.
    pub async fn get_proof(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Vec<NamespaceProof>> {
        Ok(self
            .ctx
            .rpc
            .blob_get_proof(height, namespace, commitment)
            .await?)
    }

    /// Checks whether a blob's given commitment is included at given height and under the namespace.
    pub async fn included(
        &self,
        height: u64,
        namespace: Namespace,
        proof: &NamespaceProof,
        commitment: Commitment,
    ) -> Result<bool> {
        Ok(self
            .ctx
            .rpc
            .blob_included(height, namespace, proof, commitment)
            .await?)
    }

    /// Subscribe to published blobs from the given namespace as they are included.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    pub async fn subscribe(&self, namespace: Namespace) -> Result<Subscription<BlobsAtHeight>> {
        Ok(self.ctx.rpc.blob_subscribe(namespace).await?)
    }
}
