use std::future::{self, Future};

use celestia_proto::cosmos::tx::v1beta1::SignDoc;
use celestia_types::any::JsAny;
use celestia_types::consts::appconsts::JsAppVersion;
use celestia_types::state::auth::{JsAuthParams, JsBaseAccount};
use celestia_types::state::JsCoin;
use celestia_types::Blob;
use futures::FutureExt;
use js_sys::{BigInt, Function, Promise, Uint8Array};
use k256::ecdsa::signature::Error as SignatureError;
use k256::ecdsa::{Signature, VerifyingKey};
use lumina_utils::make_object;
use prost::Message;
use send_wrapper::SendWrapper;
use tonic_web_wasm_client::Client;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::grpc::TxPriority;
use crate::tx::{DocSigner, JsTxConfig, JsTxInfo};
use crate::{Result, TxClient};

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
        let signer = JsSigner::new(signer_fn);
        let pubkey = VerifyingKey::try_from(pubkey.to_vec().as_slice())?;
        let client = TxClient::with_grpcweb_url(url, pubkey, signer).await?;
        Ok(Self { client })
    }

    /// Query for the current minimum gas price
    #[wasm_bindgen(js_name = minGasPrice)]
    pub async fn min_gas_price(&self) -> Result<f64> {
        self.client.get_min_gas_price().await
    }

    /// estimate_gas_price takes a transaction priority and estimates the gas price based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    #[wasm_bindgen(js_name = getEstimateGasPrice)]
    pub async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64> {
        self.client.estimate_gas_price(priority).await
    }

    /// Chain id of the client
    #[wasm_bindgen(js_name = chainId, getter)]
    pub fn chain_id(&self) -> String {
        self.client.chain_id().to_string()
    }

    /// AppVersion of the client
    #[wasm_bindgen(js_name = appVersion, getter)]
    pub fn app_version(&self) -> JsAppVersion {
        self.client.app_version().into()
    }

    /// Submit blobs to the celestia network.
    ///
    /// When no `TxConfig` is provided, client will automatically calculate needed
    /// gas and update the `gasPrice`, if network agreed on a new minimal value.
    /// To enforce specific values use a `TxConfig`.
    ///
    /// # Example
    /// ```js
    /// const ns = Namespace.newV0(new Uint8Array([97, 98, 99]));
    /// const data = new Uint8Array([100, 97, 116, 97]);
    /// const blob = new Blob(ns, data, AppVersion.latest());
    ///
    /// const txInfo = await txClient.submitBlobs([blob]);
    /// await txClient.submitBlobs([blob], { gasLimit: 100000n, gasPrice: 0.02, memo: "foo" });
    /// ```
    ///
    /// # Note
    ///
    /// Provided blobs will be consumed by this method, meaning
    /// they will no longer be accessible. If this behavior is not desired,
    /// consider using `Blob.clone()`.
    ///
    /// ```js
    /// const blobs = [blob1, blob2, blob3];
    /// await txClient.submitBlobs(blobs.map(b => b.clone()));
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

    /// Submit message to the celestia network.
    ///
    /// When no `TxConfig` is provided, client will automatically calculate needed
    /// gas and update the `gasPrice`, if network agreed on a new minimal value.
    /// To enforce specific values use a `TxConfig`.
    ///
    /// # Example
    /// ```js
    /// import { Registry } from "@cosmjs/proto-signing";
    ///
    /// const registry = new Registry();
    /// const sendMsg = {
    ///   typeUrl: "/cosmos.bank.v1beta1.MsgSend",
    ///   value: {
    ///     fromAddress: "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm",
    ///     toAddress: "celestia1t52q7uqgnjfzdh3wx5m5phvma3umrq8k6tq2p9",
    ///     amount: [{ denom: "utia", amount: "10000" }],
    ///   },
    /// };
    /// const sendMsgAny = registry.encodeAsAny(sendMsg);
    ///
    /// const txInfo = await txClient.submitMessage(sendMsgAny);
    /// ```
    #[wasm_bindgen(js_name = submitMessage)]
    pub async fn submit_message(
        &self,
        message: JsAny,
        tx_config: Option<JsTxConfig>,
    ) -> Result<JsTxInfo> {
        let tx_config = tx_config.map(Into::into).unwrap_or_default();
        let tx = self.client.submit_message(message, tx_config).await?;
        Ok(tx.into())
    }

    // cosmos.auth

    /// Get auth params
    #[wasm_bindgen(js_name = getAuthParams)]
    pub async fn get_auth_params(&self) -> Result<JsAuthParams> {
        self.client.get_auth_params().await.map(Into::into)
    }

    /// Get account
    #[wasm_bindgen(js_name = getAccount)]
    pub async fn get_account(&self, account: &str) -> Result<JsBaseAccount> {
        self.client
            .get_account(&account.parse()?)
            .await
            .map(Into::into)
    }

    /// Get accounts
    #[wasm_bindgen(js_name = getAccounts)]
    pub async fn get_accounts(&self) -> Result<Vec<JsBaseAccount>> {
        self.client
            .get_accounts()
            .await
            .map(|accs| accs.into_iter().map(Into::into).collect())
    }

    // cosmos.bank

    /// Get balance of coins with given denom
    #[wasm_bindgen(js_name = getBalance)]
    pub async fn get_balance(&self, address: &str, denom: &str) -> Result<JsCoin> {
        self.client
            .get_balance(&address.parse()?, denom)
            .await
            .map(Into::into)
    }

    /// Get balance of all coins
    #[wasm_bindgen(js_name = getAllBalances)]
    pub async fn get_all_balances(&self, address: &str) -> Result<Vec<JsCoin>> {
        self.client
            .get_all_balances(&address.parse()?)
            .await
            .map(|coins| coins.into_iter().map(Into::into).collect())
    }

    /// Get balance of all spendable coins
    #[wasm_bindgen(js_name = getSpendableBalances)]
    pub async fn get_spendable_balances(&self, address: &str) -> Result<Vec<JsCoin>> {
        self.client
            .get_spendable_balances(&address.parse()?)
            .await
            .map(|coins| coins.into_iter().map(Into::into).collect())
    }

    /// Get total supply
    #[wasm_bindgen(js_name = getTotalSupply)]
    pub async fn get_total_supply(&self) -> Result<Vec<JsCoin>> {
        self.client
            .get_total_supply()
            .await
            .map(|coins| coins.into_iter().map(Into::into).collect())
    }

    // TODO:
    //  - cosmos.base.node
    //  - cosmos.base.tendermint
    //  - cosmos.tx
    //  - celestia.blob
    //  - celestia.core.tx
}

/// A helper to encode the SignDoc with protobuf to get bytes to sign directly.
#[wasm_bindgen(js_name = protoEncodeSignDoc)]
pub fn proto_encode_sign_doc(sign_doc: JsSignDoc) -> Vec<u8> {
    SignDoc::from(sign_doc).encode_to_vec()
}

/// Signer that uses a javascript function for signing.
pub struct JsSigner {
    signer_fn: SendWrapper<JsSignerFn>,
}

impl JsSigner {
    fn new(signer_fn: JsSignerFn) -> Self {
        Self {
            signer_fn: SendWrapper::new(signer_fn),
        }
    }
}

impl DocSigner for JsSigner {
    fn try_sign(&self, doc: SignDoc) -> impl Future<Output = Result<Signature, SignatureError>> {
        let msg = JsSignDoc::from(doc);

        let sig_or_promise = match self.signer_fn.call1(&JsValue::null(), &msg) {
            Ok(ret) => ret,
            Err(e) => {
                let err = format!("Error calling signer fn: {e:?}");
                let err = SignatureError::from_source(err);
                return future::ready(Err(err)).boxed();
            }
        };

        if !sig_or_promise.has_type::<Promise>() {
            return future::ready(try_into_signature(sig_or_promise)).boxed();
        }

        // if signer_fn is async, await it
        let promise = sig_or_promise.unchecked_into::<Promise>();
        let future = SendWrapper::new(JsFuture::from(promise));
        async move {
            let ret = future.await.map_err(|e| {
                let err = format!("Error awaiting signer promise: {e:?}");
                SignatureError::from_source(err)
            })?;
            try_into_signature(ret)
        }
        .boxed()
    }
}

fn try_into_signature(val: JsValue) -> Result<Signature, SignatureError> {
    let sig = val.dyn_into::<Uint8Array>().map_err(|orig| {
        let err = format!(
            "Signature must be Uint8Array, found: {}",
            orig.js_typeof().as_string().expect("typeof returns string")
        );
        SignatureError::from_source(err)
    })?;

    Signature::from_slice(&sig.to_vec()).map_err(SignatureError::from_source)
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
  accountNumber: bigint;
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
