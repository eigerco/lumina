use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::time::Duration;

use bytes::Bytes;
use celestia_proto::cosmos::crypto::secp256k1;
use celestia_types::blob::{Blob, MsgPayForBlobs, RawBlobTx, RawMsgPayForBlobs};
use celestia_types::hash::Hash;
use celestia_types::state::auth::BaseAccount;
use celestia_types::state::{
    Address, AuthInfo, ErrorCode, Fee, ModeInfo, RawTx, RawTxBody, SignerInfo, Sum,
};
use celestia_types::{AppVersion, Height};
use http_body::Body;
use k256::ecdsa::signature::{Error as SignatureError, Signer};
use k256::ecdsa::{Signature, VerifyingKey};
use lumina_utils::time::Interval;
use prost::{Message, Name};
use tendermint::chain::Id;
use tendermint::PublicKey;
use tendermint_proto::google::protobuf::Any;
use tendermint_proto::Protobuf;
use tokio::sync::{Mutex, MutexGuard};
use tonic::body::BoxBody;
use tonic::client::GrpcService;

use crate::grpc::{
    Account, BroadcastMode, GasEstimate, GrpcClient, StdError, TxPriority, TxStatus,
};
use crate::{Error, Result};

pub use celestia_proto::cosmos::tx::v1beta1::SignDoc;

#[cfg(feature = "uniffi")]
uniffi::use_remote_type!(celestia_types::Hash);

// Multiplier used to adjust the gas limit given by gas estimation service
const DEFAULT_GAS_MULTIPLIER: f64 = 1.1;

// source https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
const BLOB_TX_TYPE_ID: &str = "BLOB";

/// A client for submitting messages and transactions to celestia.
///
/// Client handles management of the accounts sequence (nonce), thus
/// it should be the only party submitting transactions signed with
/// given account. Using e.g. two distinct clients with the same account
/// will make them invalidate each others nonces.
pub struct TxClient<T, S> {
    client: GrpcClient<T>,

    // NOTE: in future we might want a map of accounts
    // and something like .add_account()
    account: Mutex<Account>,
    pubkey: VerifyingKey,
    signer: S,

    app_version: AppVersion,
    chain_id: Id,
}

impl<T, S> TxClient<T, S>
where
    T: GrpcService<BoxBody> + Clone,
    T::Error: Into<StdError>,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    S: DocSigner,
{
    /// Create a new transaction client.
    pub async fn new(
        transport: T,
        account_address: &Address,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> Result<Self> {
        let client = GrpcClient::new(transport);
        let account = client.get_account(account_address).await?;
        if let Some(pubkey) = account.pub_key {
            if pubkey != PublicKey::Secp256k1(account_pubkey) {
                return Err(Error::PublicKeyMismatch);
            }
        };
        let account = Mutex::new(account);

        let block = client.get_latest_block().await?;
        let app_version = block.header.version.app;
        let app_version = AppVersion::from_u64(app_version)
            .ok_or(celestia_types::Error::UnsupportedAppVersion(app_version))?;
        let chain_id = block.header.chain_id;

        Ok(Self {
            client,
            signer,
            account,
            pubkey: account_pubkey,
            app_version,
            chain_id,
        })
    }

    /// Submit given message to celestia network.
    ///
    /// When no gas price is specified through config, it will automatically
    /// handle updating client's gas price when consensus updates minimal
    /// gas price.
    ///
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{TxClient, TxConfig};
    /// use celestia_proto::cosmos::bank::v1beta1::MsgSend;
    /// use celestia_types::state::{AccAddress, Coin};
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let public_key = *signing_key.verifying_key();
    /// let address = AccAddress::new(public_key.into()).into();
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = TxClient::with_url(grpc_url, &address, public_key, signing_key)
    ///     .await
    ///     .unwrap();
    ///
    /// let msg = MsgSend {
    ///     from_address: address.to_string(),
    ///     to_address: "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".to_string(),
    ///     amount: vec![Coin::utia(12345).into()],
    /// };
    ///
    /// tx_client
    ///     .submit_message(msg.clone(), TxConfig::default())
    ///     .await
    ///     .unwrap();
    /// # }
    /// ```
    pub async fn submit_message<M>(&self, message: M, cfg: TxConfig) -> Result<TxInfo>
    where
        M: IntoAny,
    {
        let tx_body = RawTxBody {
            messages: vec![message.into_any()],
            memo: cfg.memo.clone().unwrap_or_default(),
            ..RawTxBody::default()
        };

        let mut retries = 0;
        let (tx_hash, sequence) = loop {
            match self
                .sign_and_broadcast_tx(tx_body.clone(), cfg.clone())
                .await
            {
                Ok(resp) => break resp,
                Err(Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _)) if retries < 3 => {
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        };
        self.confirm_tx(tx_hash, sequence).await
    }

    /// Submit given blobs to celestia network.
    ///
    /// When no gas price is specified through config, it will automatically
    /// handle updating client's gas price when consensus updates minimal
    /// gas price.
    ///
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{TxClient, TxConfig};
    /// use celestia_types::state::{AccAddress, Coin};
    /// use celestia_types::{AppVersion, Blob};
    /// use celestia_types::nmt::Namespace;
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let public_key = *signing_key.verifying_key();
    /// let address = AccAddress::new(public_key.into()).into();
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = TxClient::with_url(grpc_url, &address, public_key, signing_key)
    ///     .await
    ///     .unwrap();
    ///
    /// let ns = Namespace::new_v0(b"abcd").unwrap();
    /// let blob = Blob::new(ns, "some data".into(), AppVersion::V3).unwrap();
    ///
    /// tx_client
    ///     .submit_blobs(&[blob], TxConfig::default())
    ///     .await
    ///     .unwrap();
    /// # }
    /// ```
    pub async fn submit_blobs(&self, blobs: &[Blob], cfg: TxConfig) -> Result<TxInfo> {
        if blobs.is_empty() {
            return Err(Error::TxEmptyBlobList);
        }
        for blob in blobs {
            blob.validate(self.app_version)?;
        }

        let mut retries = 0;
        let (tx_hash, sequence) = loop {
            match self
                .sign_and_broadcast_blobs(blobs.to_vec(), cfg.clone())
                .await
            {
                Ok(resp) => break resp,
                Err(Error::TxBroadcastFailed(_, ErrorCode::InsufficientFee, _)) if retries < 3 => {
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        };
        self.confirm_tx(tx_hash, sequence).await
    }

    /// Query for the current minimum gas price
    pub async fn get_min_gas_price(&self) -> Result<f64> {
        self.client.get_min_gas_price().await
    }

    /// Get client's chain id
    pub fn chain_id(&self) -> &Id {
        &self.chain_id
    }

    /// Get client's app version
    pub fn app_version(&self) -> AppVersion {
        self.app_version
    }

    /// compute gas limit and gas price according to provided TxConfig for serialised
    /// transaction, potentially calling gas estimation service
    async fn calculate_transaction_gas_params(
        &self,
        tx_body: &RawTxBody,
        cfg: &TxConfig,
        account: &BaseAccount,
    ) -> Result<(u64, f64)> {
        Ok(match (cfg.gas_limit, cfg.gas_price) {
            (Some(gas_limit), Some(gas_price)) => (gas_limit, gas_price),
            (Some(gas_limit), None) => {
                let gas_price = self.client.estimate_gas_price(cfg.priority).await?;
                (gas_limit, gas_price)
            }
            (None, maybe_gas_price) => {
                let tx = sign_tx(
                    tx_body.clone(),
                    self.chain_id.clone(),
                    account,
                    &self.pubkey,
                    &self.signer,
                    0,
                    1,
                )
                .await?;
                let GasEstimate { price, usage } = self
                    .client
                    .estimate_gas_price_and_usage(cfg.priority, tx.encode_to_vec())
                    .await?;
                let gas_limit = (usage as f64 * DEFAULT_GAS_MULTIPLIER) as u64;
                (gas_limit, maybe_gas_price.unwrap_or(price))
            }
        })
    }

    async fn sign_and_broadcast_tx(&self, tx: RawTxBody, cfg: TxConfig) -> Result<(Hash, u64)> {
        let account = self.account.lock().await;
        let sign_tx = |tx, gas, fee| {
            sign_tx(
                tx,
                self.chain_id.clone(),
                &account,
                &self.pubkey,
                &self.signer,
                gas,
                fee,
            )
        };

        let (gas_limit, gas_price) = self
            .calculate_transaction_gas_params(&tx, &cfg, &account)
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil();
        let tx = sign_tx(tx, gas_limit, fee as u64).await?;

        self.broadcast_tx_with_account(tx.encode_to_vec(), account)
            .await
    }

    async fn sign_and_broadcast_blobs(
        &self,
        blobs: Vec<Blob>,
        cfg: TxConfig,
    ) -> Result<(Hash, u64)> {
        // lock the account; tx signing and broadcast must be atomic
        // because node requires all transactions to be sequenced by account.sequence
        let account = self.account.lock().await;

        let pfb = MsgPayForBlobs::new(&blobs, account.address.clone())?;
        let pfb = RawTxBody {
            messages: vec![RawMsgPayForBlobs::from(pfb).into_any()],
            memo: cfg.memo.clone().unwrap_or_default(),
            ..RawTxBody::default()
        };

        let (gas_limit, gas_price) = self
            .calculate_transaction_gas_params(&pfb, &cfg, &account)
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil() as u64;
        let tx = sign_tx(
            pfb,
            self.chain_id.clone(),
            &account,
            &self.pubkey,
            &self.signer,
            gas_limit,
            fee,
        )
        .await?;

        let blobs = blobs.into_iter().map(Into::into).collect();
        let blob_tx = RawBlobTx {
            tx: tx.encode_to_vec(),
            blobs,
            type_id: BLOB_TX_TYPE_ID.to_string(),
        };

        self.broadcast_tx_with_account(blob_tx.encode_to_vec(), account)
            .await
    }

    async fn broadcast_tx_with_account(
        &self,
        tx: Vec<u8>,
        mut account: MutexGuard<'_, Account>,
    ) -> Result<(Hash, u64)> {
        let resp = self.client.broadcast_tx(tx, BroadcastMode::Sync).await?;

        if resp.code != ErrorCode::Success {
            return Err(Error::TxBroadcastFailed(
                resp.txhash,
                resp.code,
                resp.raw_log,
            ));
        }

        let tx_sequence = account.sequence;
        account.sequence += 1;

        Ok((resp.txhash, tx_sequence))
    }

    async fn confirm_tx(&self, hash: Hash, sequence: u64) -> Result<TxInfo> {
        let mut interval = Interval::new(Duration::from_millis(500)).await;

        loop {
            let tx_status = self.client.tx_status(hash).await?;
            match tx_status.status {
                TxStatus::Pending => interval.tick().await,
                TxStatus::Committed => {
                    if tx_status.execution_code == ErrorCode::Success {
                        return Ok(TxInfo {
                            hash,
                            height: tx_status.height,
                        });
                    } else {
                        return Err(Error::TxExecutionFailed(
                            hash,
                            tx_status.execution_code,
                            tx_status.error,
                        ));
                    }
                }
                // node will treat this transaction like if it never happened, so
                // we need to revert the account's sequence to the one of evicted tx.
                // all transactions that were already submitted after this one will fail
                // due to incorrect sequence number.
                TxStatus::Evicted => {
                    let mut acc = self.account.lock().await;
                    acc.sequence = sequence;
                    return Err(Error::TxEvicted(hash));
                }
                // this case should never happen for node that accepted a broadcast
                // however we handle it the same as evicted for extra safety
                TxStatus::Unknown => {
                    let mut acc = self.account.lock().await;
                    acc.sequence = sequence;
                    return Err(Error::TxNotFound(hash));
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<S> TxClient<tonic::transport::Channel, S>
where
    S: DocSigner,
{
    /// Create a new client connected to the given `url` with default
    /// settings of [`tonic::transport::Channel`].
    pub async fn with_url(
        url: impl Into<String>,
        account_address: &Address,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> Result<Self> {
        let transport = tonic::transport::Endpoint::from_shared(url.into())?.connect_lazy();
        Self::new(transport, account_address, account_pubkey, signer).await
    }
}

#[cfg(target_arch = "wasm32")]
impl<S> TxClient<tonic_web_wasm_client::Client, S>
where
    S: DocSigner,
{
    /// Create a new client connected to the given `url` with default
    /// settings of [`tonic_web_wasm_client::Client`].
    pub async fn with_grpcweb_url(
        url: impl Into<String>,
        account_address: &Address,
        account_pubkey: VerifyingKey,
        signer: S,
    ) -> Result<Self> {
        let transport = tonic_web_wasm_client::Client::new(url.into());
        Self::new(transport, account_address, account_pubkey, signer).await
    }
}

impl<T, S> Deref for TxClient<T, S> {
    type Target = GrpcClient<T>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl<T, S> fmt::Debug for TxClient<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TxClient { .. }")
    }
}

/// Signer capable of producing ecdsa signature using secp256k1 curve.
pub trait DocSigner {
    /// Try to sign the provided sign doc.
    fn try_sign(&self, doc: SignDoc) -> impl Future<Output = Result<Signature, SignatureError>>;
}

impl<T> DocSigner for T
where
    T: Signer<Signature>,
{
    async fn try_sign(&self, doc: SignDoc) -> Result<Signature, SignatureError> {
        let bytes = doc.encode_to_vec();
        self.try_sign(&bytes)
    }
}

/// Value convertion into protobuf's Any
pub trait IntoAny {
    /// Converts itself into protobuf's Any type
    fn into_any(self) -> Any;
}

impl<T> IntoAny for T
where
    T: Name,
{
    fn into_any(self) -> Any {
        Any {
            type_url: T::type_url(),
            value: self.encode_to_vec(),
        }
    }
}

/// A result of correctly submitted transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TxInfo {
    /// Hash of the transaction.
    pub hash: Hash,
    /// Height at which transaction was submitted.
    pub height: Height,
}

/// Configuration for the transaction.
#[derive(Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TxConfig {
    /// Custom gas limit for the transaction (in `utia`). By default, client will
    /// query gas estimation service to get estimate gas limit.
    pub gas_limit: Option<u64>,
    /// Custom gas price for fee calculation. By default, client will query gas
    /// estimation service to get gas price estimate.
    pub gas_price: Option<f64>,
    /// Memo for the transaction
    pub memo: Option<String>,
    /// Priority of the transaction, used with gas estimation service
    pub priority: TxPriority,
}

impl TxConfig {
    /// Attach gas limit to this config.
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Attach gas price to this config.
    pub fn with_gas_price(mut self, gas_price: f64) -> Self {
        self.gas_price = Some(gas_price);
        self
    }

    /// Attach memo to this config.
    pub fn with_memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }

    /// Specify transaction priority to be used when using gas estimation service
    pub fn with_priority(mut self, priority: TxPriority) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use wasm_bindgen::{prelude::*, JsCast};

    use super::{TxConfig, TxInfo, TxPriority};
    use crate::utils::make_object;

    #[wasm_bindgen(typescript_custom_section)]
    const _: &str = "
    /**
     * Transaction info
     */
    export interface TxInfo {
      /**
       * Hash of the transaction.
       */
      hash: string;
      /**
       * Height at which transaction was submitted.
       */
      height: bigint;
    }

    /**
     * Transaction priority, if not provided default TxConfig uses medium priority.
     */
    export interface TxPriority {
      /**
       * Estimated gas price is the value at the end of the lowest 10% of gas prices from the last 5 blocks.
       */
      Low = 1,
      /**
       * Estimated gas price is the mean of all gas prices from the last 5 blocks.
       */
      Medium = 2,
      /**
       * Estimated gas price is the price at the start of the top 10% of transactions’ gas prices from the last 5 blocks.
       */
      High = 3,
    }

    /**
     * Transaction config.
     */
    export interface TxConfig {
      /**
       * Custom gas limit for the transaction (in `utia`). By default, client will
       * query gas estimation service to get estimate gas limit.
       */
      gasLimit?: bigint; // utia
      /**
       * Custom gas price for fee calculation. By default, client will query gas
       * estimation service to get gas price estimate.
       */
      gasPrice?: number;
      /**
       * Memo for the transaction
       */
      memo?: string;
      /**
       * Priority of the transaction, used with gas estimation service
       */
      priority?: TxPriority;
    }
    ";

    #[wasm_bindgen]
    extern "C" {
        /// TxInfo exposed to javascript
        #[wasm_bindgen(typescript_type = "TxInfo")]
        pub type JsTxInfo;

        /// TxPriority exposed to javascript
        #[wasm_bindgen(typescript_type = "TxPriority")]
        pub type JsTxPriority;

        /// TxConfig exposed to javascript
        #[wasm_bindgen(typescript_type = "TxConfig")]
        pub type JsTxConfig;

        #[wasm_bindgen(method, getter, js_name = gasLimit)]
        pub fn gas_limit(this: &JsTxConfig) -> Option<u64>;

        #[wasm_bindgen(method, getter, js_name = gasPrice)]
        pub fn gas_price(this: &JsTxConfig) -> Option<f64>;

        #[wasm_bindgen(method, getter, js_name = memo)]
        pub fn memo(this: &JsTxConfig) -> Option<String>;

        #[wasm_bindgen(method, getter, js_name = priority)]
        pub fn priority(this: &JsTxConfig) -> Option<TxPriority>;
    }

    impl From<TxInfo> for JsTxInfo {
        fn from(value: TxInfo) -> JsTxInfo {
            let obj = make_object!(
                "hash" => value.hash.to_string().into(),
                "height" => js_sys::BigInt::from(value.height.value())
            );

            obj.unchecked_into()
        }
    }

    impl From<JsTxConfig> for TxConfig {
        fn from(value: JsTxConfig) -> TxConfig {
            TxConfig {
                gas_limit: value.gas_limit(),
                gas_price: value.gas_price(),
                memo: value.memo(),
                priority: value.priority().unwrap_or(TxPriority::Medium),
            }
        }
    }
}

/// Sign `tx_body` and the transaction metadata as the `base_account` using `signer`
pub async fn sign_tx(
    tx_body: RawTxBody,
    chain_id: Id,
    base_account: &BaseAccount,
    verifying_key: &VerifyingKey,
    signer: &impl DocSigner,
    gas_limit: u64,
    fee: u64,
) -> Result<RawTx> {
    // From https://github.com/celestiaorg/cosmos-sdk/blob/v1.25.0-sdk-v0.46.16/proto/cosmos/tx/signing/v1beta1/signing.proto#L24
    const SIGNING_MODE_INFO: ModeInfo = ModeInfo {
        sum: Sum::Single { mode: 1 },
    };

    let public_key = secp256k1::PubKey {
        key: verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    };

    let public_key_as_any = Any {
        type_url: secp256k1::PubKey::type_url(),
        value: public_key.encode_to_vec(),
    };

    let mut fee = Fee::new(fee, gas_limit);
    fee.payer = Some(base_account.address.clone());

    let auth_info = AuthInfo {
        signer_infos: vec![SignerInfo {
            public_key: Some(public_key_as_any),
            mode_info: SIGNING_MODE_INFO,
            sequence: base_account.sequence,
        }],
        fee,
    };

    let doc = SignDoc {
        body_bytes: tx_body.encode_to_vec(),
        auth_info_bytes: auth_info.clone().encode_vec(),
        chain_id: chain_id.into(),
        account_number: base_account.account_number,
    };
    let signature = signer.try_sign(doc).await?;

    Ok(RawTx {
        auth_info: Some(auth_info.into()),
        body: Some(tx_body),
        signatures: vec![signature.to_bytes().to_vec()],
    })
}
