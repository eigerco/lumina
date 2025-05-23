use std::sync::Arc;

use celestia_types::state::Address;
use celestia_types::Blob;
use k256::ecdsa::signature::Error as K256Error;
use k256::ecdsa::{Signature as DocSignature, VerifyingKey};
use tendermint_proto::google::protobuf::Any;
use tonic::transport::Channel;
use uniffi::{Object, Record};
use prost::Message;

use crate::tx::TxInfo;
use crate::{DocSigner, SignDoc, TxConfig};

type Result<T, E = TransactionClientError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TransactionClientError {
    #[error("grpc error: {msg}")]
    GrpcError { msg: String },

    #[error("invalid account public key")]
    InvalidAccountPublicKey { msg: String },

    #[error("invalid account id")]
    InvalidAccountId,

    #[error("error while signing: {msg}")]
    SigningError { msg: String }
}

#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait UniffiSigner: Sync + Send {
    async fn sign(&self, doc: SignDoc) -> Result<UniffiSignature, TransactionClientError>;
}

struct UniffiSignerBox(pub Arc<dyn UniffiSigner>);

#[derive(Record)]
pub struct UniffiSignature {
    pub bytes: Vec<u8>,
}

#[derive(Object)]
pub struct TxClient {
    client: crate::TxClient<Channel, UniffiSignerBox>,
}

#[derive(Record)]
pub struct AnyMsg {
    pub r#type: String,
    pub value: Vec<u8>,
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
            .map_err(|e| TransactionClientError::InvalidAccountPublicKey { msg: e.to_string() })?;

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

impl From<AnyMsg> for Any {
    fn from(value: AnyMsg) -> Self {
        Any {
            type_url: value.r#type,
            value: value.value,
        }
    }
}

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


impl From<DocSignature> for UniffiSignature {
    fn from(value: DocSignature) -> Self {
        UniffiSignature {
            bytes: value.to_vec(),
        }
    }
}

impl TryFrom<UniffiSignature> for DocSignature {
    type Error = TransactionClientError;

    fn try_from(value: UniffiSignature) -> std::result::Result<Self, Self::Error> {
        DocSignature::from_slice(&value.bytes).map_err(|e| TransactionClientError::SigningError {
            msg: format!("invalid signature {e}"),
        })
    }
}

impl From<crate::Error> for TransactionClientError {
    fn from(value: crate::Error) -> Self {
        TransactionClientError::GrpcError {
            msg: value.to_string(),
        }
    }
}

#[uniffi::export]
fn proto_encode_sign_doc(sign_doc: SignDoc) -> Vec<u8> {
    sign_doc.encode_to_vec()
}

#[uniffi::export]
fn parse_bech32_address(bech32_address: String) -> Result<Address> {
    bech32_address.parse().map_err(|_| TransactionClientError::InvalidAccountId)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn new_tx_client(
        url: String,
        account_address: &Address,
        account_pubkey: Vec<u8>,
        signer: Arc<dyn UniffiSigner>,
) -> Result<TxClient> {
    TxClient::new(url, account_address, account_pubkey, signer).await
}

