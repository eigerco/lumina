use js_sys::Uint8Array;
use k256::ecdsa::VerifyingKey;
use wasm_bindgen::prelude::*;

use crate::signer::{JsSigner, JsSignerFn};
use crate::GrpcClientBuilderError;

mod grpc_client;

use grpc_client::GrpcClient;

/// Builder for [`GrpcClient`] and [`TxClient`].
///
/// Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).
///
/// # Keyless client example
///
/// ```js
/// const client = await GrpcClient
///   .withUrl("http://127.0.0.1:18080")
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
/// const client = await GrpcClient
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
/// const client = await GrpcClient
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
    /// Set the `url` of the grpc-web server to connect to
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = "withUrl")]
    pub fn with_url(self, url: String) -> Self {
        Self {
            inner: self.inner.url(url),
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

    /// Appends ascii metadata to all requests made by the client.
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = withMetadata)]
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        Self {
            inner: self.inner.metadata(&key, &value),
        }
    }

    /// Appends binary metadata to all requests made by the client.
    ///
    /// Keys for binary metadata must have `-bin` suffix.
    ///
    /// Note that this method **consumes** builder and returns updated instance of it.
    /// Make sure to re-assign it if you keep builder in a variable.
    #[wasm_bindgen(js_name = withMetadataBin)]
    pub fn with_metadata_bin(mut self, key: String, value: Uint8Array) -> Self {
        Self {
            inner: self.inner.metadata_bin(&key, &value.to_vec()),
        }
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
