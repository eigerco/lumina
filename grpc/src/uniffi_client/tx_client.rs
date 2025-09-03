//! GRPC transaction client wrapper for uniffi

use std::sync::Arc;

use celestia_types::{AppVersion, Blob};
use k256::ecdsa::signature::Error as K256Error;
use k256::ecdsa::{Signature as DocSignature, VerifyingKey};
use prost::Message;
use tendermint_proto::google::protobuf::Any;
use tonic::transport::Channel;
use uniffi::{Object, Record};

use crate::tx::TxInfo;
use crate::{DocSigner, IntoProtobufAny, SignDoc, TxConfig};

type Result<T, E = TransactionClientError> = std::result::Result<T, E>;

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

/// Trait that implements signing the transaction.
///
/// Example usage:
/// ```swift
/// // uses 21-DOT-DEV/swift-secp256k1
/// final class StaticSigner : UniffiSigner {
///     let sk : P256K.Signing.PrivateKey
///
///     init(sk: P256K.Signing.PrivateKey) {
///         self.sk = sk
///     }
///
///     func sign(doc: SignDoc) async throws -> UniffiSignature {
///         let messageData = protoEncodeSignDoc(signDoc: doc);
///         let signature = try! sk.signature(for: messageData)
///         return try! UniffiSignature (bytes: signature.compactRepresentation)
///     }
/// }
/// ```
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait UniffiSigner: Sync + Send {
    /// sign provided `SignDoc` using secp256k1. Use helper proto_encode_sign_doc to
    /// get canonical protobuf byte encoding of the message.
    async fn sign(&self, doc: SignDoc) -> Result<UniffiSignature, TransactionClientError>;
}

struct UniffiSignerBox(pub Arc<dyn UniffiSigner>);

/// Message signature
#[derive(Record)]
pub struct UniffiSignature {
    /// signature bytes
    pub bytes: Vec<u8>,
}

/// Celestia GRPC transaction client
#[derive(Object)]
pub struct TxClient {
    client: crate::TxClient<Channel, UniffiSignerBox>,
}

/// Any contains an arbitrary serialized protocol buffer message along with a URL that
/// describes the type of the serialized message.
#[derive(Record)]
pub struct AnyMsg {
    /// A URL/resource name that uniquely identifies the type of the serialized protocol
    /// buffer message. This string must contain at least one “/” character. The last
    /// segment of the URL’s path must represent the fully qualified name of the type
    /// (as in path/google.protobuf.Duration). The name should be in a canonical form
    /// (e.g., leading “.” is not accepted).
    pub r#type: String,
    /// Must be a valid serialized protocol buffer of the above specified type.
    pub value: Vec<u8>,
}

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

impl IntoProtobufAny for AnyMsg {
    fn into_any(self) -> Any {
        Any {
            type_url: self.r#type,
            value: self.value,
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
