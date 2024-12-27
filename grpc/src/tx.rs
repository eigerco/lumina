use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use bytes::Bytes;
use celestia_proto::cosmos::crypto::secp256k1;
use celestia_proto::cosmos::tx::v1beta1::SignDoc;
use celestia_types::blob::{Blob, MsgPayForBlobs, RawBlobTx, RawMsgPayForBlobs};
use celestia_types::consts::appconsts;
use celestia_types::hash::Hash;
use celestia_types::state::auth::BaseAccount;
use celestia_types::state::{
    Address, AuthInfo, ErrorCode, Fee, ModeInfo, RawTx, RawTxBody, SignerInfo, Sum,
};
use celestia_types::{AppVersion, Height};
use http_body::Body;
use k256::ecdsa::signature::{Error as SignatureError, Signer};
use k256::ecdsa::{Signature, VerifyingKey};
use prost::{Message, Name};
use regex::Regex;
use tendermint::chain::Id;
use tendermint::PublicKey;
use tendermint_proto::google::protobuf::Any;
use tendermint_proto::Protobuf;
use tokio::sync::{Mutex, MutexGuard};
use tonic::body::BoxBody;
use tonic::client::GrpcService;

use crate::grpc::Account;
use crate::grpc::TxStatus;
use crate::grpc::{GrpcClient, StdError};
use crate::utils::Interval;
use crate::{Error, Result};

pub use celestia_proto::cosmos::tx::v1beta1::BroadcastMode;

// source https://github.com/celestiaorg/celestia-app/blob/v3.0.2/x/blob/types/payforblob.go#L21
// PFBGasFixedCost is a rough estimate for the "fixed cost" in the gas cost
// formula: gas cost = gas per byte * bytes per share * shares occupied by
// blob + "fixed cost". In this context, "fixed cost" accounts for the gas
// consumed by operations outside the blob's GasToConsume function (i.e.
// signature verification, tx size, read access to accounts).
//
// Since the gas cost of these operations is not easy to calculate, linear
// regression was performed on a set of observed data points to derive an
// approximate formula for gas cost. Assuming gas per byte = 8 and bytes per
// share = 512, we can solve for "fixed cost" and arrive at 65,000. gas cost
// = 8 * 512 * number of shares occupied by the blob + 65,000 has a
// correlation coefficient of 0.996. To be conservative, we round up "fixed
// cost" to 75,000 because the first tx always takes up 10,000 more gas than
// subsequent txs.
const PFB_GAS_FIXED_COST: u64 = 75000;
// BytesPerBlobInfo is a rough estimation for the amount of extra bytes in
// information a blob adds to the size of the underlying transaction.
const BYTES_PER_BLOB_INFO: u64 = 70;
// source https://github.com/celestiaorg/celestia-app/blob/v3.0.2/pkg/appconsts/initial_consts.go#L20
// DefaultMinGasPrice is the default min gas price that gets set in the app.toml file.
// The min gas price acts as a filter. Transactions below that limit will not pass
// a nodes `CheckTx` and thus not be proposed by that node.
const DEFAULT_MIN_GAS_PRICE: f64 = 0.002; // utia
const DEFAULT_GAS_MULTIPLIER: f64 = 1.1;
// source https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
const BLOB_TX_TYPE_ID: &str = "BLOB";

/// A result of correctly submitted transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxInfo {
    /// Hash of the transaction.
    pub hash: Hash,
    /// Height at which transaction was submitted.
    pub height: Height,
}

/// Configuration for the transaction.
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct TxConfig {
    /// Custom gas limit for the transaction.
    pub gas_limit: Option<u64>,
    /// Custom gas price for fee calculation.
    pub gas_price: Option<f64>,
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
}

pub trait AsyncSigner {
    fn try_sign(&self, doc: SignDoc) -> impl Future<Output = Result<Signature, SignatureError>>;
}

impl<T> AsyncSigner for T
where
    T: Signer<Signature>,
{
    async fn try_sign(&self, doc: SignDoc) -> Result<Signature, SignatureError> {
        let bytes = doc.encode_to_vec();
        self.try_sign(&bytes)
    }
}

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
    gas_price: RwLock<f64>,
}

impl<T, S> TxClient<T, S>
where
    T: GrpcService<BoxBody> + Clone,
    T::Error: Into<StdError>,
    T::ResponseBody: Body<Data = Bytes> + Send + 'static,
    <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    S: AsyncSigner,
{
    /// Create a new transaction client.
    ///
    /// Public key is optional if it can be retrieved from the account.
    pub async fn new(
        client: GrpcClient<T>,
        signer: S,
        account_address: &Address,
        account_pubkey: Option<VerifyingKey>,
    ) -> Result<Self> {
        let account = client.get_account(account_address).await?;
        let pubkey = match (account.pub_key, account_pubkey) {
            (Some(fetched), Some(provided)) => {
                if fetched != PublicKey::Secp256k1(provided) {
                    return Err(Error::PublicKeyMismatch);
                }
                provided
            }
            (Some(fetched), None) => {
                if let PublicKey::Secp256k1(pubkey) = fetched {
                    pubkey
                } else {
                    return Err(Error::KeyAlgorithmNotSupported);
                }
            }
            (None, Some(provided)) => provided,
            (None, None) => return Err(Error::PublicKeyMissing),
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
            pubkey,
            app_version,
            chain_id,
            gas_price: RwLock::new(DEFAULT_MIN_GAS_PRICE),
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
    /// use celestia_grpc::{GrpcClient, TxClient, TxConfig};
    /// use celestia_proto::cosmos::bank::v1beta1::MsgSend;
    /// use celestia_types::state::{AccAddress, Coin};
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let public_key = *signing_key.verifying_key();
    /// let address = AccAddress::new(public_key.into()).into();
    /// let grpc = GrpcClient::with_url("celestia-app-grpc-url:9090").unwrap();
    ///
    /// let tx_client = TxClient::new(grpc, signing_key, &address, Some(public_key))
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
        M: Name,
    {
        let tx_body = RawTxBody {
            messages: vec![into_any(message)],
            ..RawTxBody::default()
        };

        let mut is_retry = false;
        let (tx_hash, sequence) = loop {
            match self.sign_and_broadcast_tx(tx_body.clone(), cfg).await {
                Ok(resp) => break resp,
                Err(e) if !is_retry => {
                    if self.maybe_update_gas_price(&e, cfg)? {
                        is_retry = true;
                        continue;
                    }
                    return Err(e);
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
    /// use celestia_grpc::{GrpcClient, TxClient, TxConfig};
    /// use celestia_types::state::{AccAddress, Coin};
    /// use celestia_types::{AppVersion, Blob};
    /// use celestia_types::nmt::Namespace;
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let public_key = *signing_key.verifying_key();
    /// let address = AccAddress::new(public_key.into()).into();
    /// let grpc = GrpcClient::with_url("celestia-app-grpc-url:9090").unwrap();
    ///
    /// let tx_client = TxClient::new(grpc, signing_key, &address, Some(public_key))
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

        let mut is_retry = false;
        let (tx_hash, sequence) = loop {
            match self.sign_and_broadcast_blobs(blobs.to_vec(), cfg).await {
                Ok(resp) => break resp,
                Err(e) if !is_retry => {
                    if self.maybe_update_gas_price(&e, cfg)? {
                        is_retry = true;
                        continue;
                    }
                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        };
        self.confirm_tx(tx_hash, sequence).await
    }

    /// Get current gas price used by the client
    pub fn gas_price(&self) -> f64 {
        *self.gas_price.read().expect("lock poisoned")
    }

    /// Set current gas price used by the client
    pub fn set_gas_price(&self, gas_price: f64) {
        *self.gas_price.write().expect("lock poisoned") = gas_price;
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

        let gas_limit = if let Some(gas_limit) = cfg.gas_limit {
            gas_limit
        } else {
            // simulate the gas that would be used by transaction
            // fee should be at least 1 as it affects calculation
            let tx = sign_tx(tx.clone(), 0, 1).await?;
            let gas_info = self.client.simulate(tx.encode_to_vec()).await?;
            (gas_info.gas_used as f64 * DEFAULT_GAS_MULTIPLIER) as u64
        };

        let gas_price = cfg.gas_price.unwrap_or(self.gas_price());
        let fee = (gas_limit as f64 * gas_price).ceil();
        let tx = sign_tx(tx, gas_limit, fee as u64).await?;

        self.broadcast_tx_with_account(tx.encode_to_vec(), account, gas_limit)
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
            messages: vec![into_any(RawMsgPayForBlobs::from(pfb))],
            ..RawTxBody::default()
        };

        let gas_limit = cfg
            .gas_limit
            .unwrap_or_else(|| estimate_gas(&blobs, self.app_version, DEFAULT_GAS_MULTIPLIER));
        let gas_price = cfg.gas_price.unwrap_or(self.gas_price());
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

        println!(
            "Signed tx; sequence: {}, signature: {:?}",
            account.sequence,
            tx.signatures.first().unwrap()
        );

        let blobs = blobs.into_iter().map(Into::into).collect();
        let blob_tx = RawBlobTx {
            tx: tx.encode_to_vec(),
            blobs,
            type_id: BLOB_TX_TYPE_ID.to_string(),
        };

        self.broadcast_tx_with_account(blob_tx.encode_to_vec(), account, gas_limit)
            .await
    }

    async fn broadcast_tx_with_account(
        &self,
        tx: Vec<u8>,
        mut account: MutexGuard<'_, Account>,
        gas_limit: u64,
    ) -> Result<(Hash, u64)> {
        let resp = self.client.broadcast_tx(tx, BroadcastMode::Sync).await?;

        if resp.code != ErrorCode::Success {
            return Err(Error::TxBroadcastFailed(
                resp.txhash,
                resp.code,
                resp.raw_log,
                gas_limit,
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
                TxStatus::Unknown => return Err(Error::TxNotFound(hash)),
                TxStatus::Evicted => {
                    // node will treat this transaction like if it never happened, so
                    // we need to revert the account's sequence to the one of evicted tx.
                    // all transactions that were already submitted after this one
                    // will fail due to incorrect sequence number.
                    let mut acc = self.account.lock().await;
                    acc.sequence = sequence;
                    return Err(Error::TxEvicted(hash));
                }
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
            }
        }
    }

    fn maybe_update_gas_price(&self, err: &Error, cfg: TxConfig) -> Result<bool> {
        let Error::TxBroadcastFailed(_, code, error, gas_limit) = err else {
            return Ok(false);
        };
        // nothing to update if we didn't use our gas internal gas price
        if cfg.gas_price.is_some() {
            return Ok(false);
        }
        if *code != ErrorCode::InsufficientFee {
            return Ok(false);
        }

        let Some((got_fee, want_fee)) = parse_insufficient_gas_err(error) else {
            return Err(Error::UpdatingGasPriceFailed(format!(
                "Couldn't parse required fee from error: {err}"
            )));
        };
        if want_fee == 0.0 {
            return Err(Error::UpdatingGasPriceFailed(format!(
                "Wanted fee is 0: {err}"
            )));
        }

        let new_gas_price = if self.gas_price() == 0.0 || got_fee == 0.0 {
            if *gas_limit == 0 {
                return Err(Error::UpdatingGasPriceFailed(format!(
                    "Cannot update gas price when gas price and limit are 0: {err}"
                )));
            }
            want_fee / *gas_limit as f64
        } else {
            want_fee / got_fee * self.gas_price()
        };

        self.set_gas_price(new_gas_price.ceil());
        Ok(true)
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

/// Sign `tx_body` and the transaction metadata as the `base_account` using `signer`
pub async fn sign_tx(
    tx_body: RawTxBody,
    chain_id: Id,
    base_account: &BaseAccount,
    verifying_key: &VerifyingKey,
    signer: &impl AsyncSigner,
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

fn estimate_gas(blobs: &[Blob], app_version: AppVersion, gas_multiplier: f64) -> u64 {
    let gas_per_blob_byte = appconsts::gas_per_blob_byte(app_version);
    let tx_size_cost_per_byte = appconsts::tx_size_cost_per_byte(app_version);

    let blobs_bytes =
        blobs.iter().map(Blob::shares_len).sum::<usize>() as u64 * appconsts::SHARE_SIZE as u64;

    let gas = blobs_bytes * gas_per_blob_byte
        + (tx_size_cost_per_byte * BYTES_PER_BLOB_INFO * blobs.len() as u64)
        + PFB_GAS_FIXED_COST;
    (gas as f64 * gas_multiplier) as u64
}

// Any::from_msg is infallible, but it yet returns result
fn into_any<M>(msg: M) -> Any
where
    M: Name,
{
    Any {
        type_url: M::type_url(),
        value: msg.encode_to_vec(),
    }
}

fn parse_insufficient_gas_err(error: &str) -> Option<(f64, f64)> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        // insufficient minimum gas price for this node; got: 50 required at least: 199.000000000000000000: insufficient fee
        Regex::new(r".*got.*?(?P<got>[0-9.]+).*required.*?(?P<want>[0-9.]+)").expect("valid regex")
    });
    let caps = RE.captures(error)?;
    let got: f64 = caps["got"].parse().ok()?;
    let want: f64 = caps["want"].parse().ok()?;

    Some((got, want))
}
