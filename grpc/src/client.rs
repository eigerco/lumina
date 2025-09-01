#![allow(unused)]

use std::error::Error as StdError;
use std::fmt;
use std::pin::Pin;

use ::tendermint::chain::Id;
use bytes::Bytes;
use celestia_types::any::IntoProtobufAny;
use dyn_clone::DynClone;
use futures::future::BoxFuture;
use k256::ecdsa::VerifyingKey;
use lumina_utils::time::Interval;
use prost::Message;
use signature::Keypair;
use std::time::Duration;
use tokio::sync::{MappedMutexGuard, Mutex, MutexGuard};
use tonic::body::Body as TonicBody;
use tonic::client::GrpcService;
use tonic::codegen::Service;

use celestia_grpc_macros::grpc_method;
use celestia_proto::celestia::blob::v1::query_client::QueryClient as BlobQueryClient;
use celestia_proto::celestia::core::v1::gas_estimation::gas_estimator_client::GasEstimatorClient;
use celestia_proto::celestia::core::v1::tx::tx_client::TxClient as TxStatusClient;
use celestia_proto::cosmos::auth::v1beta1::query_client::QueryClient as AuthQueryClient;
use celestia_proto::cosmos::bank::v1beta1::query_client::QueryClient as BankQueryClient;
use celestia_proto::cosmos::base::node::v1beta1::service_client::ServiceClient as ConfigServiceClient;
use celestia_proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient;
use celestia_proto::cosmos::staking::v1beta1::query_client::QueryClient as StakingQueryClient;
use celestia_proto::cosmos::tx::v1beta1::service_client::ServiceClient as TxServiceClient;
use celestia_types::blob::{BlobParams, MsgPayForBlobs, RawBlobTx, RawMsgPayForBlobs};
use celestia_types::block::Block;
use celestia_types::consts::appconsts;
use celestia_types::hash::Hash;
use celestia_types::state::auth::{Account, AuthParams, BaseAccount};
use celestia_types::state::{
    AbciQueryResponse, PageRequest, QueryDelegationResponse, QueryRedelegationsResponse,
    QueryUnbondingDelegationResponse, RawTxBody, ValAddress,
};
use celestia_types::state::{
    AccAddress, Address, AddressTrait, Coin, ErrorCode, TxResponse, BOND_DENOM,
};
use celestia_types::{AppVersion, Blob, ExtendedHeader};

use crate::abci_proofs::ProofChain;
use crate::boxed::{BoxedTransport, ConditionalSend};
use crate::builder::GrpcClientBuilder;
use crate::grpc::{
    BroadcastMode, GasEstimate, GasInfo, GetTxResponse, TxPriority, TxStatus, TxStatusResponse,
};
use crate::signer::{sign_tx, DispatchedDocSigner, KeypairExt};
use crate::tx::TxInfo;
use crate::{Error, Result, TxConfig};

// source https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
const BLOB_TX_TYPE_ID: &str = "BLOB";
// Multiplier used to adjust the gas limit given by gas estimation service
const DEFAULT_GAS_MULTIPLIER: f64 = 1.1;

struct AccountBits {
    account: Account,
    app_version: AppVersion,
    chain_id: Id,
}

pub(crate) struct SignerBits {
    pub(crate) signer: DispatchedDocSigner,
    pub(crate) pubkey: VerifyingKey,
}

impl Keypair for SignerBits {
    type VerifyingKey = VerifyingKey;

    fn verifying_key(&self) -> Self::VerifyingKey {
        self.pubkey
    }
}

/// gRPC client for the Celestia network
///
/// Under the hood, this struct wraps tonic and does type conversion
pub struct GrpcClient {
    transport: BoxedTransport,
    account: Mutex<Option<AccountBits>>,
    signer: Option<SignerBits>,
}

impl GrpcClient {
    /// Create a new client wrapping given transport
    pub(crate) fn new(transport: BoxedTransport, signer: Option<SignerBits>) -> Self {
        Self {
            transport,
            account: Mutex::new(None),
            signer,
        }
    }

    /// Create a builder for [`GrpcClient`] connected to `url`
    pub fn with_url(url: impl Into<String>) -> GrpcClientBuilder {
        GrpcClientBuilder::with_url(url)
    }

    // cosmos.auth

    /// Get auth params
    #[grpc_method(AuthQueryClient::params)]
    async fn get_auth_params(&self) -> Result<AuthParams>;

    /// Get account
    #[grpc_method(AuthQueryClient::account)]
    async fn get_account(&self, account: &AccAddress) -> Result<Account>;

    /// Get accounts
    #[grpc_method(AuthQueryClient::accounts)]
    async fn get_accounts(&self) -> Result<Vec<Account>>;

    // cosmos.bank

    /// Get balance of coins with [`BOND_DENOM`] for the given address, together with a proof,
    /// and verify the returned balance against the corresponding block's [`AppHash`].
    ///
    /// NOTE: the balance returned is the balance reported by the parent block of
    /// the provided header. This is due to the fact that for block N, the block's
    /// [`AppHash`] is the result of applying the previous block's transaction list.
    ///
    /// [`AppHash`]: ::tendermint::hash::AppHash
    pub async fn get_verified_balance(
        &self,
        address: &Address,
        header: &ExtendedHeader,
    ) -> Result<Coin> {
        // construct the key for querying account's balance from bank state
        let mut prefixed_account_key = Vec::with_capacity(1 + 1 + appconsts::SIGNER_SIZE + 4);
        prefixed_account_key.push(0x02); // balances prefix
        prefixed_account_key.push(address.as_bytes().len() as u8); // address length
        prefixed_account_key.extend_from_slice(address.as_bytes()); // address
        prefixed_account_key.extend_from_slice(BOND_DENOM.as_bytes()); // denom

        // abci queries are possible only from 2nd block, but we can rely on the node to
        // return an error if that is the case. We only want to prevent height == 0 here,
        // because that will make node interpret that as a network head. It would also fail,
        // on proof verification, but it would be harder to debug as the message would just
        // say that computed root is different than expected.
        let height = 1.max(header.height().value().saturating_sub(1));

        let response = self
            .abci_query(&prefixed_account_key, "store/bank/key", height, true)
            .await?;
        if response.code != ErrorCode::Success {
            return Err(Error::AbciQuery(response.code, response.log));
        }

        // If account doesn't exist yet, just return 0
        if response.value.is_empty() {
            return Ok(Coin::utia(0));
        }

        // NOTE: don't put `ProofChain` directly in the AbciQueryResponse, because
        // it supports only small subset of proofs that are required for the balance
        // queries
        let proof: ProofChain = response.proof_ops.unwrap_or_default().try_into()?;
        proof.verify_membership(
            &header.header.app_hash,
            [prefixed_account_key.as_slice(), b"bank"],
            &response.value,
        )?;

        let amount = str::from_utf8(&response.value)
            .map_err(|_| Error::FailedToParseResponse)?
            .parse()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(Coin::utia(amount))
    }

    /// Get balance of coins with given denom
    #[grpc_method(BankQueryClient::balance)]
    async fn get_balance(&self, address: &Address, denom: impl Into<String>) -> Result<Coin>;

    /// Get balance of all coins
    #[grpc_method(BankQueryClient::all_balances)]
    async fn get_all_balances(&self, address: &Address) -> Result<Vec<Coin>>;

    /// Get balance of all spendable coins
    #[grpc_method(BankQueryClient::spendable_balances)]
    async fn get_spendable_balances(&self, address: &Address) -> Result<Vec<Coin>>;

    /// Get total supply
    #[grpc_method(BankQueryClient::total_supply)]
    async fn get_total_supply(&self) -> Result<Vec<Coin>>;

    // cosmos.base.node

    /// Get Minimum Gas price
    #[grpc_method(ConfigServiceClient::config)]
    async fn get_min_gas_price(&self) -> Result<f64>;

    // cosmos.base.tendermint

    /// Get latest block
    #[grpc_method(TendermintServiceClient::get_latest_block)]
    async fn get_latest_block(&self) -> Result<Block>;

    /// Get block by height
    #[grpc_method(TendermintServiceClient::get_block_by_height)]
    async fn get_block_by_height(&self, height: i64) -> Result<Block>;

    /// Issue a direct ABCI query to the application
    #[grpc_method(TendermintServiceClient::abci_query)]
    async fn abci_query(
        &self,
        data: impl AsRef<[u8]>,
        path: impl Into<String>,
        height: u64,
        prove: bool,
    ) -> Result<AbciQueryResponse>;

    // cosmos.tx

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::broadcast_tx)]
    async fn broadcast_tx(&self, tx_bytes: Vec<u8>, mode: BroadcastMode) -> Result<TxResponse>;

    /// Get Tx
    #[grpc_method(TxServiceClient::get_tx)]
    async fn get_tx(&self, hash: Hash) -> Result<GetTxResponse>;

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::simulate)]
    async fn simulate(&self, tx_bytes: Vec<u8>) -> Result<GasInfo>;

    // cosmos.staking

    /// Retrieves the delegation information between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::delegation)]
    async fn query_delegation(
        &self,
        delegator_address: &AccAddress,
        validator_address: &ValAddress,
    ) -> Result<QueryDelegationResponse>;

    /// Retrieves the unbonding status between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::unbonding_delegation)]
    async fn query_unbonding(
        &self,
        delegator_address: &AccAddress,
        validator_address: &ValAddress,
    ) -> Result<QueryUnbondingDelegationResponse>;

    /// Retrieves the status of the redelegations between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::redelegations)]
    async fn query_redelegations(
        &self,
        delegator_address: &AccAddress,
        src_validator_address: &ValAddress,
        dest_validator_address: &ValAddress,
        pagination: Option<PageRequest>,
    ) -> Result<QueryRedelegationsResponse>;

    // celestia.blob

    /// Get blob params
    #[grpc_method(BlobQueryClient::params)]
    async fn get_blob_params(&self) -> Result<BlobParams>;

    // celestia.core.tx

    /// Get status of the transaction
    #[grpc_method(TxStatusClient::tx_status)]
    async fn tx_status(&self, hash: Hash) -> Result<TxStatusResponse>;

    // celestia.core.gas_estimation

    /// Estimate gas price for given transaction priority based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    #[grpc_method(GasEstimatorClient::estimate_gas_price)]
    async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64>;

    /// Estimate gas price for transaction with given priority and estimate gas usage
    /// for the provided serialised transaction.
    ///
    /// The gas price estimation is based on the gas prices of the transactions
    /// in the last five blocks.
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    ///
    /// The gas used is estimated using the state machine simulation.
    #[grpc_method(GasEstimatorClient::estimate_gas_price_and_usage)]
    async fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> Result<GasEstimate>;

    /// Submit given message to celestia network.
    ///
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{GrpcClientBuilder, TxConfig};
    /// use celestia_proto::cosmos::bank::v1beta1::MsgSend;
    /// use celestia_types::state::{Address, Coin};
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let address = Address::from_account_veryfing_key(*signing_key.verifying_key());
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = GrpcClientBuilder::with_url(grpc_url)
    ///     .with_signer_keypair(signing_key)
    ///     .build()
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
        M: IntoProtobufAny,
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
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{GrpcClientBuilder, TxConfig};
    /// use celestia_types::state::{Address, Coin};
    /// use celestia_types::{AppVersion, Blob};
    /// use celestia_types::nmt::Namespace;
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let address = Address::from_account_veryfing_key(*signing_key.verifying_key());
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = GrpcClientBuilder::with_url(grpc_url)
    ///     .with_signer_keypair(signing_key)
    ///     .build()
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
        let app_version = self.app_version().await?;
        for blob in blobs {
            blob.validate(app_version)?;
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

    async fn load_account(&self) -> Result<(MappedMutexGuard<'_, AccountBits>, &SignerBits)> {
        let signer = self.signer.as_ref().ok_or(Error::NoAccount)?;
        let mut account_guard = self.account.lock().await;

        if account_guard.is_none() {
            let account = self.get_account(&signer.address()).await?;

            let block = self.get_latest_block().await?;
            let app_version = block.header.version.app;
            let app_version = AppVersion::from_u64(app_version)
                .ok_or(celestia_types::Error::UnsupportedAppVersion(app_version))?;
            let chain_id = block.header.chain_id;

            *account_guard = Some(AccountBits {
                account,
                app_version,
                chain_id,
            })
        }
        let mapped_guard = MutexGuard::map(account_guard, |acc| {
            acc.as_mut().expect("account data present")
        });

        Ok((mapped_guard, signer))
    }

    /// compute gas limit and gas price according to provided TxConfig for serialised
    /// transaction, potentially calling gas estimation service
    async fn calculate_transaction_gas_params(
        &self,
        tx_body: &RawTxBody,
        cfg: &TxConfig,
        chain_id: Id,
        account: &BaseAccount,
    ) -> Result<(u64, f64)> {
        let signer = self.signer.as_ref().ok_or(Error::NoAccount)?;

        Ok(match (cfg.gas_limit, cfg.gas_price) {
            (Some(gas_limit), Some(gas_price)) => (gas_limit, gas_price),
            (Some(gas_limit), None) => {
                let gas_price = self.estimate_gas_price(cfg.priority).await?;
                (gas_limit, gas_price)
            }
            (None, maybe_gas_price) => {
                let tx = sign_tx(
                    tx_body.clone(),
                    chain_id,
                    account,
                    &signer.verifying_key(),
                    &signer.signer,
                    0,
                    1,
                )
                .await?;

                let GasEstimate { price, usage } = self
                    .estimate_gas_price_and_usage(cfg.priority, tx.encode_to_vec())
                    .await?;
                let gas_limit = (usage as f64 * DEFAULT_GAS_MULTIPLIER) as u64;
                (gas_limit, maybe_gas_price.unwrap_or(price))
            }
        })
    }

    async fn sign_and_broadcast_blobs(
        &self,
        blobs: Vec<Blob>,
        cfg: TxConfig,
    ) -> Result<(Hash, u64)> {
        // lock the account; tx signing and broadcast must be atomic
        // because node requires all transactions to be sequenced by account.sequence
        let (account, signer) = self.load_account().await?;

        let pfb = MsgPayForBlobs::new(&blobs, account.account.address.clone())?;
        let pfb = RawTxBody {
            messages: vec![RawMsgPayForBlobs::from(pfb).into_any()],
            memo: cfg.memo.clone().unwrap_or_default(),
            ..RawTxBody::default()
        };

        let (gas_limit, gas_price) = self
            .calculate_transaction_gas_params(
                &pfb,
                &cfg,
                account.chain_id.clone(),
                &account.account,
            )
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil() as u64;
        let tx = sign_tx(
            pfb,
            account.chain_id.clone(),
            &account.account,
            &signer.verifying_key(),
            &signer.signer,
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

    async fn sign_and_broadcast_tx(&self, tx: RawTxBody, cfg: TxConfig) -> Result<(Hash, u64)> {
        let (account, signer) = self.load_account().await?;

        let (gas_limit, gas_price) = self
            .calculate_transaction_gas_params(&tx, &cfg, account.chain_id.clone(), &account.account)
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil();
        let tx = sign_tx(
            tx,
            account.chain_id.clone(),
            &account.account,
            &signer.verifying_key(),
            &signer.signer,
            gas_limit,
            fee as u64,
        )
        .await?;

        self.broadcast_tx_with_account(tx.encode_to_vec(), account)
            .await
    }

    async fn broadcast_tx_with_account(
        &self,
        tx: Vec<u8>,
        mut account: MappedMutexGuard<'_, AccountBits>,
    ) -> Result<(Hash, u64)> {
        let resp = self.broadcast_tx(tx, BroadcastMode::Sync).await?;

        if resp.code != ErrorCode::Success {
            return Err(Error::TxBroadcastFailed(
                resp.txhash,
                resp.code,
                resp.raw_log,
            ));
        }

        let tx_sequence = account.account.sequence;
        account.account.sequence += 1;

        Ok((resp.txhash, tx_sequence))
    }

    async fn confirm_tx(&self, hash: Hash, sequence: u64) -> Result<TxInfo> {
        let mut interval = Interval::new(Duration::from_millis(500)).await;

        loop {
            let tx_status = self.tx_status(hash).await?;
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
                    acc.as_mut().expect("account data present").account.sequence = sequence;
                    return Err(Error::TxEvicted(hash));
                }
                // this case should never happen for node that accepted a broadcast
                // however we handle it the same as evicted for extra safety
                TxStatus::Unknown => {
                    let mut acc = self.account.lock().await;
                    acc.as_mut().expect("account data present").account.sequence = sequence;
                    return Err(Error::TxNotFound(hash));
                }
            }
        }
    }

    /// Get client's app version
    ///
    /// Note that this function _may_ try to load the account information from the network and as
    /// such requires signed to be set up.
    pub async fn app_version(&self) -> Result<AppVersion> {
        let (account, _) = self.load_account().await?;
        Ok(account.app_version)
    }

    /// Get client's chain id
    ///
    /// Note that this function _may_ try to load the account information from the network and as
    /// such requires signed to be set up.
    pub async fn chain_id(&self) -> Result<Id> {
        let (account, _) = self.load_account().await?;
        Ok(account.chain_id.clone())
    }
}

impl fmt::Debug for GrpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GrpcClient { .. }")
    }
}
