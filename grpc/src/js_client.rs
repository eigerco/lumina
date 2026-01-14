use std::time::Duration;

use js_sys::Uint8Array;
use k256::ecdsa::VerifyingKey;
use wasm_bindgen::prelude::*;

use crate::GrpcClientBuilderError;
use crate::signer::{JsSigner, JsSignerFn};

mod grpc_client;

use grpc_client::GrpcClient;

/// Configuration specific to a single URL endpoint.
///
/// This includes HTTP/2 headers (metadata) and timeout that will be applied
/// to all requests made to this endpoint.
///
/// # Example
///
/// ```js
/// const config = new EndpointConfig()
///   .withMetadata("authorization", "Bearer token")
///   .withTimeout(5000);
///
/// const client = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080", config)
///   .build();
/// ```
#[wasm_bindgen]
pub struct EndpointConfig {
    inner: crate::EndpointConfig,
}

#[wasm_bindgen]
impl EndpointConfig {
    /// Create a new, empty endpoint configuration.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: crate::EndpointConfig::new(),
        }
    }

    /// Appends ASCII metadata (HTTP/2 header) to requests made to this endpoint.
    ///
    /// Note that this method **consumes** the config and returns an updated instance.
    #[wasm_bindgen(js_name = "withMetadata")]
    pub fn with_metadata(self, key: String, value: String) -> Self {
        Self {
            inner: self.inner.metadata(key, value),
        }
    }

    /// Appends binary metadata to requests made to this endpoint.
    ///
    /// Keys must have `-bin` suffix.
    ///
    /// Note that this method **consumes** the config and returns an updated instance.
    #[wasm_bindgen(js_name = "withMetadataBin")]
    pub fn with_metadata_bin(self, key: String, value: Uint8Array) -> Self {
        Self {
            inner: self.inner.metadata_bin(key, value.to_vec()),
        }
    }

    /// Sets the request timeout in milliseconds for this endpoint.
    ///
    /// Note that this method **consumes** the config and returns an updated instance.
    #[wasm_bindgen(js_name = "withTimeout")]
    pub fn with_timeout(self, timeout_ms: u64) -> Self {
        Self {
            inner: self.inner.timeout(Duration::from_millis(timeout_ms)),
        }
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for [`GrpcClient`] and [`TxClient`].
///
/// Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).
///
/// # Keyless client example
///
/// ```js
/// const client = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080")
///   .build()
///
/// // With timeout:
/// const config = new EndpointConfig().withTimeout(5000);
/// const client = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080", config)
///   .build()
/// ```
///
/// # Transaction client examples
///
/// ## Example with noble/curves
/// ```js
/// import { secp256k1 } from "@noble/curves/secp256k1";
///
/// const privKey = "fdc8ac75dfa1c142dbcba77938a14dd03078052ce0b49a529dcf72a9885a3abb";
/// const pubKey = secp256k1.getPublicKey(privKey);
///
/// const signer = (signDoc) => {
///   const bytes = protoEncodeSignDoc(signDoc);
///   const sig = secp256k1.sign(bytes, privKey, { prehash: true });
///   return sig.toCompactRawBytes();
/// };
///
/// const client = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080")
///   .withPubkeyAndSigner(pubKey, signer)
///   .build();
/// ```
///
/// ## Example with leap wallet
/// ```js
/// await window.leap.enable("mocha-4")
/// const keys = await window.leap.getKey("mocha-4")
///
/// const signer = (signDoc) => {
///   return window.leap.signDirect("mocha-4", keys.bech32Address, signDoc, { preferNoSetFee: true })
///     .then(sig => Uint8Array.from(atob(sig.signature.signature), c => c.charCodeAt(0)))
/// }
///
/// const client = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080")
///   .withPubkeyAndSigner(keys.pubKey, signer)
///   .build()
/// ```
#[wasm_bindgen]
pub struct GrpcClientBuilder {
    inner: crate::GrpcClientBuilder,
}

#[wasm_bindgen]
impl GrpcClientBuilder {
    /// Set the `url` of the grpc-web server to connect to with optional configuration.
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = "withUrl")]
    pub fn with_url(self, url: String, config: Option<EndpointConfig>) -> Self {
        let config = config.map(|c| c.inner).unwrap_or_default();
        Self {
            inner: self.inner.url(url, config),
        }
    }

    /// Add multiple URL endpoints at once for fallback support with default configuration.
    ///
    /// When multiple endpoints are configured, the client will automatically
    /// fall back to the next endpoint if a network-related error occurs.
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = "withUrls")]
    pub fn with_urls(self, urls: Vec<String>) -> Self {
        Self {
            inner: self.inner.urls(urls),
        }
    }

    /// Add public key and signer to the client being built
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = withPubkeyAndSigner)]
    pub fn with_pubkey_and_signer(
        self,
        account_pubkey: Uint8Array,
        signer_fn: JsSignerFn,
    ) -> Result<Self, GrpcClientBuilderError> {
        let signer = JsSigner::new(signer_fn);
        let account_pubkey = VerifyingKey::try_from(account_pubkey.to_vec().as_slice())
            .map_err(|_| GrpcClientBuilderError::InvalidPublicKey)?;
        Ok(Self {
            inner: self.inner.pubkey_and_signer(account_pubkey, signer),
        })
    }

    /// build gRPC client
    pub fn build(self) -> Result<GrpcClient, GrpcClientBuilderError> {
        Ok(self.inner.build()?.into())
    }
}

impl From<crate::GrpcClientBuilder> for GrpcClientBuilder {
    fn from(inner: crate::GrpcClientBuilder) -> Self {
        Self { inner }
    }
}
