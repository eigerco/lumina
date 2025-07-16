use std::sync::Arc;

use celestia_rpc::BlobClient;
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::{Blob, Commitment};
use futures_util::{Stream, TryStreamExt};

use crate::client::Context;
use crate::tx::{TxConfig, TxInfo};
use crate::Result;

pub use celestia_rpc::blob::BlobsAtHeight;

/// Blob API for quering bridge nodes.
pub struct BlobApi {
    ctx: Arc<Context>,
}

impl BlobApi {
    pub(crate) fn new(ctx: Arc<Context>) -> BlobApi {
        BlobApi { ctx }
    }

    /// Submit given blobs to celestia network.
    ///
    /// When no gas price is specified through config, it will automatically
    /// handle updating client's gas price when consensus updates minimal
    /// gas price.
    ///
    /// # Notes
    ///
    /// This is the same as [`StateApi::submit_pay_for_blob`].
    ///
    /// # Example
    /// ```no_run
    /// # use celestia_client::{Client, Result};
    /// # use celestia_client::tx::TxConfig;
    /// # const RPC_URL: &str = "http://localhost:26658";
    /// # const GRPC_URL : &str = "http://localhost:19090";
    /// # async fn docs() -> Result<()> {
    /// use celestia_types::nmt::Namespace;
    /// use celestia_types::state::{Address, Coin};
    /// use celestia_types::{AppVersion, Blob};
    ///
    /// let client = Client::builder()
    ///     .rpc_url(RPC_URL)
    ///     .grpc_url(GRPC_URL)
    ///     .plaintext_private_key("...")?
    ///     .build()
    ///     .await?;
    ///
    /// let ns = Namespace::new_v0(b"abcd").unwrap();
    /// let blob = Blob::new(ns, "some data".into(), AppVersion::V3).unwrap();
    ///
    /// client.blob().submit(&[blob], TxConfig::default()).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`StateApi::submit_pay_for_blob`]: crate::api::StateApi::submit_pay_for_blob
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
        let blob = self.ctx.rpc.blob_get(height, namespace, commitment).await?;
        let app_version = self.ctx.get_header_validated(height).await?.app_version()?;

        blob.validate_with_commitment(&commitment, app_version)?;

        Ok(blob)
    }

    /// Retrieves all blobs under the given namespaces and height.
    pub async fn get_all(
        &self,
        height: u64,
        namespaces: &[Namespace],
    ) -> Result<Option<Vec<Blob>>> {
        let Some(blobs) = self.ctx.rpc.blob_get_all(height, namespaces).await? else {
            return Ok(None);
        };

        let app_version = self.ctx.get_header_validated(height).await?.app_version()?;

        for blob in &blobs {
            blob.validate(app_version)?;
        }

        Ok(Some(blobs))
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
    // TODO: Should we validate blobs?
    pub async fn subscribe(
        &self,
        namespace: Namespace,
    ) -> Result<impl Stream<Item = Result<BlobsAtHeight>>> {
        Ok(self
            .ctx
            .rpc
            .blob_subscribe(namespace)
            .await?
            .map_err(Into::into))
    }
}
