use std::sync::Arc;

use celestia_types::state::Address;
use celestia_types::Blob;
use k256::ecdsa::signature::Error as K256Error;
use k256::ecdsa::{Signature as DocSignature, VerifyingKey};
use tendermint_proto::google::protobuf::Any;
use tonic::transport::Channel;
use uniffi::{Object, Record};

use crate::tx::TxInfo;
use crate::{DocSigner, SignDoc, TxConfig};

type Result<T, E = GrpcError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GrpcError {
    #[error("grpc error: {msg}")]
    GrpcError { msg: String },

    #[error("invalid account public key")]
    InvalidAccountPublicKey { msg: String },
}

impl From<crate::Error> for GrpcError {
    fn from(value: crate::Error) -> Self {
        GrpcError::GrpcError {
            msg: value.to_string(),
        }
    }
}

#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait UniffiSigner: Sync + Send {
    async fn sign(&self, doc: SignDoc) -> Result<Signature, SigningError>;
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SigningError {
    #[error("signing error: {msg}")]
    SigningError { msg: String },
}

#[derive(Object)]
pub struct TxClient {
    client: crate::TxClient<Channel, UniffiSignerBox>,
}

#[uniffi::export(async_runtime = "tokio")]
impl TxClient {
    #[uniffi::constructor]
    pub async fn new(
        url: String,
        account_address: &Address,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
    ) -> Result<Self> {
        let vk = VerifyingKey::from_sec1_bytes(&account_pubkey)
            .map_err(|e| GrpcError::InvalidAccountPublicKey { msg: e.to_string() })?;

        let signer = UniffiSignerBox(signer);

        let client = crate::TxClient::with_url(url, account_address, vk, signer).await?;

        Ok(TxClient { client })
    }

    /// Last gas price fetched by the client
    pub fn last_seen_gas_price(&self) -> f64 {
        self.client.last_seen_gas_price()
    }

    /// AppVersion of the client
    pub fn app_version(&self) -> u64 {
        self.client.app_version().as_u64()
    }

    pub async fn submit_blobs(&self, blobs: Vec<Blob>, config: Option<TxConfig>) -> Result<TxInfo> {
        let config = config.unwrap_or_default();
        Ok(self.client.submit_blobs(&blobs, config).await?)
    }

    pub async fn submit_message(
        &self,
        message: AnyMsg,
        config: Option<TxConfig>,
    ) -> Result<TxInfo> {
        let config = config.unwrap_or_default();
        Ok(self
            .client
            .submit_message(Any::from(message), config)
            .await?)
    }
}

#[derive(Record)]
pub struct AnyMsg {
    pub r#type: String,
    pub value: Vec<u8>,
}

impl From<AnyMsg> for Any {
    fn from(value: AnyMsg) -> Self {
        Any {
            type_url: value.r#type,
            value: value.value,
        }
    }
}

struct UniffiSignerBox(pub Arc<dyn UniffiSigner>);

impl DocSigner for UniffiSignerBox {
    async fn try_sign(
        &self,
        doc: SignDoc,
    ) -> Result<tendermint::signature::Secp256k1Signature, K256Error> {
        match self.0.sign(doc).await {
            Ok(s) => s.try_into().map_err(K256Error::from_source),
            Err(e) => Err(K256Error::from_source(e)),
        }
    }
}

#[derive(Record)]
pub struct Signature {
    bytes: Vec<u8>,
}

impl From<DocSignature> for Signature {
    fn from(value: DocSignature) -> Self {
        Signature {
            bytes: value.to_vec(),
        }
    }
}

impl TryFrom<Signature> for DocSignature {
    type Error = SigningError;

    fn try_from(value: Signature) -> std::result::Result<Self, Self::Error> {
        DocSignature::from_slice(&value.bytes).map_err(|e| SigningError::SigningError {
            msg: format!("invalid signature {e}"),
        })
    }
}
