use celestia_proto::cosmos::tx::v1beta1::SignDoc;
use celestia_types::Blob;
use js_sys::{BigInt, Function, Promise, Uint8Array};
use k256::ecdsa::signature::Error as SignatureError;
use k256::ecdsa::{Signature, VerifyingKey};
use prost::Message;
use tonic_web_wasm_client::Client;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::tx::{DocSigner, JsTxConfig, JsTxInfo};
use crate::utils::make_object;
use crate::{GrpcClient, Result, TxClient};

/// Celestia grpc transaction client.
#[wasm_bindgen(js_name = "TxClient")]
pub struct JsClient {
    client: TxClient<Client, JsSigner>,
}

#[wasm_bindgen(js_class = "TxClient")]
impl JsClient {
    /// Create a new transaction client with the specified account.
    ///
    /// Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).
    ///
    /// # Example with noble/curves
    /// ```js
    /// import { secp256k1 } from "@noble/curves/secp256k1";
    ///
    /// const address = "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm";
    /// const privKey = "fdc8ac75dfa1c142dbcba77938a14dd03078052ce0b49a529dcf72a9885a3abb";
    /// const pubKey = secp256k1.getPublicKey(privKey);
    ///
    /// const signer = (signDoc) => {
    ///   const bytes = protoEncodeSignDoc(signDoc);
    ///   const sig = secp256k1.sign(bytes, privKey, { prehash: true });
    ///   return sig.toCompactRawBytes();
    /// };
    ///
    /// const txClient = await new TxClient("http://127.0.0.1:18080", address, pubKey, signer);
    /// ```
    ///
    /// # Example with leap wallet
    /// ```js
    /// await window.leap.enable("mocha-4")
    /// const keys = await window.leap.getKey("mocha-4")
    ///
    /// const signer = (signDoc) => {
    ///   return window.leap.signDirect("mocha-4", keys.bech32Address, signDoc, { preferNoSetFee: true })
    ///     .then(sig => Uint8Array.from(atob(sig.signature.signature), c => c.charCodeAt(0)))
    /// }
    ///
    /// const tx_client = await new TxClient("http://127.0.0.1:18080", keys.bech32Address, keys.pubKey, signer)
    /// ```
    #[wasm_bindgen(constructor)]
    pub async fn new(
        url: String,
        bech32_address: String,
        pubkey: Uint8Array,
        signer_fn: JsSignerFn,
    ) -> Result<JsClient> {
        let grpc = GrpcClient::with_grpcweb_url(url);
        let signer = JsSigner { signer_fn };
        let address = bech32_address.parse()?;
        let pubkey = VerifyingKey::try_from(pubkey.to_vec().as_slice())?;
        let client = TxClient::new(grpc, signer, &address, Some(pubkey)).await?;
        Ok(Self { client })
    }

    /// Get current gas price of a client
    #[wasm_bindgen(js_name = gasPrice, getter)]
    pub fn get_gas_price(&self) -> f64 {
        self.client.gas_price()
    }

    /// Set current gas price of a client
    #[wasm_bindgen(js_name = gasPrice, setter)]
    pub fn set_gas_price(&self, price: f64) {
        self.client.set_gas_price(price)
    }

    /// Submit blobs to celestia network.
    ///
    /// Provided blobs will be consumed by this method, meaning
    /// they will no longer be accessible. If this behavior is not desired,
    /// consider using `Blob.clone()`.
    ///
    /// # Example
    /// ```js
    /// const ns = Namespace.newV0(new Uint8Array([97, 98, 99]));
    /// const data = new Uint8Array([100, 97, 116, 97]);
    /// const blob = new Blob(ns, data, AppVersion.latest());
    ///
    /// const txInfo = await txClient.submitBlobs([blob]);
    /// ```
    ///
    /// When no `TxConfig` is provided, client will automatically calculate needed
    /// gas and update the `gasPrice` if network agreed on a new minimal value.
    /// To enforce specific values use a `TxConfig`.
    ///
    /// # Example
    /// ```js
    /// const txInfo = await txClient.submitBlobs([blob], { gasLimit: 100000n, gasPrice: 0.02 });
    /// ```
    #[wasm_bindgen(js_name = submitBlobs)]
    pub async fn submit_blobs(
        &self,
        blobs: Vec<Blob>,
        tx_config: Option<JsTxConfig>,
    ) -> Result<JsTxInfo> {
        let tx_config = tx_config.map(Into::into).unwrap_or_default();
        let tx = self.client.submit_blobs(&blobs, tx_config).await?;
        Ok(tx.into())
    }
}

/// A helper to encode the SignDoc with protobuf to get bytes to sign directly.
#[wasm_bindgen(js_name = protoEncodeSignDoc)]
pub fn proto_encode_sign_doc(sign_doc: JsSignDoc) -> Vec<u8> {
    SignDoc::from(sign_doc).encode_to_vec()
}

/// Signer that uses a javascript function for signing.
pub struct JsSigner {
    signer_fn: JsSignerFn,
}

impl DocSigner for JsSigner {
    async fn try_sign(&self, doc: SignDoc) -> Result<Signature, SignatureError> {
        let msg = JsSignDoc::from(doc);

        let mut res = self.signer_fn.call1(&JsValue::null(), &msg).map_err(|e| {
            let err = format!("Error calling signer fn: {e:?}");
            SignatureError::from_source(err)
        })?;

        // if signer_fn is async, await it
        if res.has_type::<Promise>() {
            let promise = res.unchecked_into::<Promise>();
            res = JsFuture::from(promise).await.map_err(|e| {
                let err = format!("Error awaiting signer promise: {e:?}");
                SignatureError::from_source(err)
            })?
        }

        let sig = res.dyn_into::<Uint8Array>().map_err(|orig| {
            let err = format!(
                "Signature must be Uint8Array, found: {}",
                orig.js_typeof().as_string().expect("typeof returns string")
            );
            SignatureError::from_source(err)
        })?;

        Signature::from_slice(&sig.to_vec()).map_err(SignatureError::from_source)
    }
}

#[wasm_bindgen(typescript_custom_section)]
const _: &str = "
/**
 * A payload to be signed
 */
export interface SignDoc {
  bodyBytes: Uint8Array;
  authInfoBytes: Uint8Array;
  chainId: string;
  accountNumber: BigInt;
}

/**
 * A function that produces a signature of a payload
 */
export type SignerFn = ((arg: SignDoc) => Uint8Array) | ((arg: SignDoc) => Promise<Uint8Array>);
";

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Function, typescript_type = "SignerFn")]
    pub type JsSignerFn;

    #[wasm_bindgen(typescript_type = "SignDoc")]
    pub type JsSignDoc;

    #[wasm_bindgen(method, getter, js_name = bodyBytes)]
    pub fn body_bytes(this: &JsSignDoc) -> Vec<u8>;

    #[wasm_bindgen(method, getter, js_name = authInfoBytes)]
    pub fn auth_info_bytes(this: &JsSignDoc) -> Vec<u8>;

    #[wasm_bindgen(method, getter, js_name = chainId)]
    pub fn chain_id(this: &JsSignDoc) -> String;

    #[wasm_bindgen(method, getter, js_name = accountNumber)]
    pub fn account_number(this: &JsSignDoc) -> u64;
}

impl From<JsSignDoc> for SignDoc {
    fn from(value: JsSignDoc) -> SignDoc {
        SignDoc {
            body_bytes: value.body_bytes(),
            auth_info_bytes: value.auth_info_bytes(),
            chain_id: value.chain_id(),
            account_number: value.account_number(),
        }
    }
}

impl From<SignDoc> for JsSignDoc {
    fn from(value: SignDoc) -> JsSignDoc {
        let obj = make_object!(
            "bodyBytes" => Uint8Array::from(value.body_bytes.as_ref()),
            "authInfoBytes" => Uint8Array::from(value.auth_info_bytes.as_ref()),
            "chainId" => value.chain_id.into(),
            "accountNumber" => BigInt::from(value.account_number)
        );

        obj.unchecked_into()
    }
}
