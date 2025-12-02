use std::fmt::{self, Debug};
use std::sync::Arc;

use celestia_grpc::GrpcClient;
use celestia_rpc::{Client as RpcClient, HeaderClient};

use crate::blob::BlobApi;
use crate::blobstream::BlobstreamApi;
use crate::fraud::FraudApi;
use crate::header::HeaderApi;
use crate::share::ShareApi;
use crate::state::StateApi;
use crate::tx::VerifyingKey;
use crate::types::ExtendedHeader;
use crate::types::state::AccAddress;
use crate::{ClientBuilder, Error, Result};

/// A high-level client for interacting with a Celestia node.
///
/// There are two modes: read-only mode and submit mode. Read-only mode requires
/// RPC and optionally gRPC endpoint, while submit mode requires both, plus a signer.
///
/// # Examples
///
/// Read-only mode:
///
/// ```no_run
/// # use celestia_client::{Client, Result};
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rpc_url("ws://localhost:26658")
///     .grpc_url("http://localhost:9090") // optional in read-only mode
///     .build()
///     .await?;
///
/// client.header().head().await?;
/// # Ok(())
/// # }
/// ```
///
/// Submit mode:
///
/// ```no_run
/// # use celestia_client::{Client, Result};
/// # use celestia_client::tx::TxConfig;
/// # async fn docs() -> Result<()> {
/// let client = Client::builder()
///     .rpc_url("ws://localhost:26658")
///     .grpc_url("http://localhost:9090")
///     .private_key_hex("393fdb5def075819de55756b45c9e2c8531a8c78dd6eede483d3440e9457d839")
///     .build()
///     .await?;
///
/// let to_address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".parse().unwrap();
/// client.state().transfer(&to_address, 12345, TxConfig::default()).await?;
/// # Ok(())
/// # }
/// ```
///
/// [`celestia-rpc`]: celestia_rpc
/// [`celestia-grpc`]: celestia_grpc
pub struct Client {
    inner: Arc<ClientInner>,
    state: StateApi,
    blob: BlobApi,
    header: HeaderApi,
    share: ShareApi,
    fraud: FraudApi,
    blobstream: BlobstreamApi,
}

pub(crate) struct ClientInner {
    pub(crate) rpc: RpcClient,
    pub(crate) grpc: Option<GrpcClient>,
    pub(crate) pubkey: Option<VerifyingKey>,
    pub(crate) chain_id: tendermint::chain::Id,
}

impl ClientInner {
    pub(crate) fn grpc(&self) -> Result<&GrpcClient> {
        self.grpc.as_ref().ok_or(Error::GrpcEndpointNotSet)
    }

    pub(crate) fn pubkey(&self) -> Result<&VerifyingKey> {
        self.pubkey.as_ref().ok_or(Error::NoAssociatedAddress)
    }

    pub(crate) fn address(&self) -> Result<AccAddress> {
        let pubkey = self.pubkey()?.to_owned();
        Ok(AccAddress::new(pubkey.into()))
    }

    pub(crate) async fn get_header_validated(&self, height: u64) -> Result<ExtendedHeader> {
        let header = self.rpc.header_get_by_height(height).await?;
        header.validate()?;
        Ok(header)
    }
}

impl Client {
    /// Returns `ClientBuilder`.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Returns chain id of the network.
    pub fn chain_id(&self) -> &tendermint::chain::Id {
        &self.inner.chain_id
    }

    /// Returns the public key of the signer.
    pub fn pubkey(&self) -> Result<VerifyingKey> {
        self.inner.pubkey().cloned()
    }

    /// Returns the address of signer.
    pub fn address(&self) -> Result<AccAddress> {
        self.inner.address()
    }

    /// Returns state API accessor.
    pub fn state(&self) -> &StateApi {
        &self.state
    }

    /// Returns blob API accessor.
    pub fn blob(&self) -> &BlobApi {
        &self.blob
    }

    /// Returns blobstream API accessor.
    pub fn blobstream(&self) -> &BlobstreamApi {
        &self.blobstream
    }

    /// Returns header API accessor.
    pub fn header(&self) -> &HeaderApi {
        &self.header
    }

    /// Returns share API accessor.
    pub fn share(&self) -> &ShareApi {
        &self.share
    }

    /// Returns fraud API accessor.
    pub fn fraud(&self) -> &FraudApi {
        &self.fraud
    }
}

impl From<ClientInner> for Client {
    fn from(value: ClientInner) -> Client {
        let inner = Arc::new(value);

        Client {
            inner: inner.clone(),
            blob: BlobApi::new(inner.clone()),
            header: HeaderApi::new(inner.clone()),
            share: ShareApi::new(inner.clone()),
            fraud: FraudApi::new(inner.clone()),
            blobstream: BlobstreamApi::new(inner.clone()),
            state: StateApi::new(inner.clone()),
        }
    }
}

impl Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Client { .. }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumina_utils::test_utils::async_test;

    use crate::test_utils::{TEST_PRIV_KEY, TEST_RPC_URL};

    #[async_test]
    async fn builder() {
        let e = Client::builder()
            .rpc_url(TEST_RPC_URL)
            .private_key_hex(TEST_PRIV_KEY)
            .build()
            .await
            .unwrap_err();
        assert!(matches!(e, Error::GrpcEndpointNotSet))
    }
}
