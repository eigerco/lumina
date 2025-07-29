use celestia_proto::cosmos::tx::v1beta1::SignDoc;
use celestia_types::any::JsAny;
use celestia_types::consts::appconsts::JsAppVersion;
use celestia_types::state::auth::{JsAuthParams, JsBaseAccount};
use celestia_types::state::JsCoin;
use celestia_types::Blob;
use js_sys::{BigInt, Function, Promise, Uint8Array};
use k256::ecdsa::signature::Error as SignatureError;
use k256::ecdsa::{Signature, VerifyingKey};
use lumina_utils::make_object;
use prost::Message;
use tonic_web_wasm_client::Client;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::grpc::TxPriority;
use crate::tx::{DocSigner, JsTxConfig, JsTxInfo};
use crate::{Result, TxClient};

/*
/// Celestia grpc transaction client.
#[wasm_bindgen(js_name = "TxClient")]
pub struct JsTxClient {
    client: TxClient<Client, JsSigner>,
}

#[wasm_bindgen(js_class = "TxClient")]
impl JsTxClient {
    /// Create a new transaction client with the specified account.
    ///
    /// Url must point to a [grpc-web proxy](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-WEB.md).
    ///
    /// # Example with noble/curves
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
    /// const txClient = await new TxClient("http://127.0.0.1:18080", pubKey, signer);
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
    /// const tx_client = await new TxClient("http://127.0.0.1:18080", keys.pubKey, signer)
    /// ```
    #[wasm_bindgen(constructor)]
    pub async fn new(url: &str, pubkey: Uint8Array, signer_fn: JsSignerFn) -> Result<JsTxClient> {
        let signer = JsSigner { signer_fn };
        let pubkey = VerifyingKey::try_from(pubkey.to_vec().as_slice())?;
        let client = TxClient::with_grpcweb_url(url, pubkey, signer).await?;
        Ok(Self { client })
    }


    // TODO:
    //  - cosmos.base.node
    //  - cosmos.base.tendermint
    //  - cosmos.tx
    //  - celestia.blob
    //  - celestia.core.tx
}
*/

