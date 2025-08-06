use js_sys::Uint8Array;
use k256::ecdsa::VerifyingKey;
use wasm_bindgen::prelude::*;

use crate::signer::{JsSigner, JsSignerFn};
use crate::Result;

mod grpc_client;

use grpc_client::GrpcClient;

type RustBuilder = crate::builder::GrpcClientBuilder<tonic_web_wasm_client::Client>;

/// Builder for [`GrpcClient`] and [`TxClient`].
///
/// Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).
///
/// # Keyless client example
///
/// ```js
/// const grpcClient = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080")
///   .buildClient()
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
/// const txClient = await GrpcClientBuilder
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
/// const txClient = await GrpcClientBuilder
///   .withUrl("http://127.0.0.1:18080")
///   .withPubkeyAndSigner(keys.pubKey, signer)
///   .buildTxClient()
/// ```
#[wasm_bindgen]
pub struct GrpcClientBuilder {
    inner: RustBuilder,
}

#[wasm_bindgen]
impl GrpcClientBuilder {
    /// Create a builder for [`GrpcClient`] connected to `url`
    #[wasm_bindgen(js_name = withUrl)]
    pub fn with_url(url: String) -> Self {
        Self {
            inner: RustBuilder::with_grpcweb_url(url),
        }
    }

    /// Add public key and signer to the client being built
    #[wasm_bindgen(js_name = withPubkeyAndSigner)]
    pub fn with_pubkey_and_signer(
        self,
        account_pubkey: Uint8Array,
        signer_fn: JsSignerFn,
    ) -> Result<Self> {
        let signer = JsSigner::new(signer_fn);
        let account_pubkey = VerifyingKey::try_from(account_pubkey.to_vec().as_slice())?;
        Ok(Self {
            inner: self.inner.with_pubkey_and_signer(account_pubkey, signer),
        })
    }

    /// build gRPC read-only client. If you need to send messages, use [`build_tx_client`]
    #[wasm_bindgen(js_name = build)]
    pub async fn build(self) -> Result<GrpcClient> {
        Ok(self
            .inner
            .build()
            .expect("client creation successful")
            .into())
    }
}
