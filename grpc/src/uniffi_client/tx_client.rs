//! GRPC transaction client wrapper for uniffi


use std::sync::Arc;

use celestia_types::state::Address;
use celestia_types::{AppVersion, Blob};
use k256::ecdsa::signature::Error as K256Error;
use k256::ecdsa::{Signature as DocSignature, VerifyingKey};
use prost::Message;
use tendermint_proto::google::protobuf::Any;
use tonic::transport::Channel;
use uniffi::{Object, Record};

use crate::tx::TxInfo;
use crate::{IntoProtobufAny, SignDoc, TxConfig};

//type Result<T, E = TransactionClientError> = std::result::Result<T, E>;

/*
/// Errors returned from TxClient
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TransactionClientError {
    /// Error returned from grpc
    #[error("grpc error: {msg}")]
    GrpcError {
        /// error message
        msg: String,
    },

    /// Invalid account public key
    #[error("invalid account public key")]
    InvalidAccountPublicKey {
        /// error message
        msg: String,
    },

    /// Invalid account id
    #[error("invalid account id: {msg}")]
    InvalidAccountId {
        ///error message
        msg: String,
    },

    /// Error occured during signing
    #[error("error while signing: {msg}")]
    SigningError {
        /// error message
        msg: String,
    },
}
*/

/*
/// Celestia GRPC transaction client
#[derive(Object)]
pub struct TxClient {
    //client: crate::TxClient<Channel, UniffiSignerBox>,
}
*/

/*
#[uniffi::export(async_runtime = "tokio")]
impl TxClient {
    /// Create a new transaction client with the specified account.
    // constructor cannot be named `new`, otherwise it doesn't show up in Kotlin ¯\_(ツ)_/¯
    #[uniffi::constructor(name = "create")]
    pub async fn new(
        url: String,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
    ) -> Result<Self> {
        let vk = VerifyingKey::from_sec1_bytes(&account_pubkey)
            .map_err(|e| TransactionClientError::InvalidAccountPublicKey { msg: e.to_string() })?;

        let signer = UniffiSignerBox(signer);

        let client = crate::TxClient::with_url(url, vk, signer).await?;

        Ok(TxClient { client })
    }

    /// Query for the current minimum gas price
    pub async fn get_min_gas_price(&self) -> Result<f64> {
        Ok(self.client.get_min_gas_price().await?)
    }

    /// AppVersion of the client
    pub fn app_version(&self) -> AppVersion {
        self.client.app_version()
    }

    /// Submit blobs to the celestia network.
    ///
    /// When no `TxConfig` is provided, client will automatically calculate needed
    /// gas and update the `gasPrice`, if network agreed on a new minimal value.
    /// To enforce specific values use a `TxConfig`.
    pub async fn submit_blobs(
        &self,
        blobs: Vec<Arc<Blob>>,
        config: Option<TxConfig>,
    ) -> Result<TxInfo> {
        let blobs = Vec::from_iter(blobs.into_iter().map(Arc::<Blob>::unwrap_or_clone));
        let config = config.unwrap_or_default();
        Ok(self.client.submit_blobs(&blobs, config).await?)
    }

    /// Submit message to the celestia network.
    ///
    /// When no `TxConfig` is provided, client will automatically calculate needed
    /// gas and update the `gasPrice`, if network agreed on a new minimal value.
    /// To enforce specific values use a `TxConfig`.
    pub async fn submit_message(
        &self,
        message: AnyMsg,
        config: Option<TxConfig>,
    ) -> Result<TxInfo> {
        let config = config.unwrap_or_default();
        Ok(self.client.submit_message(message, config).await?)
    }
}
*/

/*
impl From<crate::Error> for TransactionClientError {
    fn from(value: crate::Error) -> Self {
        TransactionClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}
*/
