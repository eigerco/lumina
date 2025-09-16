use std::fmt;
use std::sync::Arc;

use ::tendermint::chain::Id;
use celestia_types::any::IntoProtobufAny;
use k256::ecdsa::VerifyingKey;
use lumina_utils::time::Interval;
use prost::Message;
use std::time::Duration;
use tokio::sync::{MappedMutexGuard, Mutex, MutexGuard};

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
use crate::boxed::BoxedTransport;
use crate::builder::GrpcClientBuilder;
use crate::grpc::{
    AsyncGrpcCall, BroadcastMode, ConfigResponse, Context, GasEstimate, GasInfo, GetTxResponse,
    TxPriority, TxStatus, TxStatusResponse,
};
use crate::signer::{sign_tx, BoxedDocSigner};
use crate::tx::TxInfo;
use crate::{Error, Result, TxConfig};

// source https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
const BLOB_TX_TYPE_ID: &str = "BLOB";

struct AccountState {
    account: Account,
    app_version: AppVersion,
    chain_id: Id,
}

pub(crate) struct SignerConfig {
    pub(crate) signer: BoxedDocSigner,
    pub(crate) pubkey: VerifyingKey,
}

/// gRPC client for the Celestia network
///
/// Under the hood, this struct wraps tonic and does type conversion
#[derive(Clone)]
pub struct GrpcClient {
    inner: Arc<GrpcClientInner>,
}

struct GrpcClientInner {
    transport: BoxedTransport,
    account: Mutex<Option<AccountState>>,
    signer: Option<SignerConfig>,
    context: Context,
}

impl GrpcClient {
    /// Create a new client wrapping given transport
    pub(crate) fn new(
        transport: BoxedTransport,
        signer: Option<SignerConfig>,
        context: Context,
    ) -> Self {
        Self {
            inner: Arc::new(GrpcClientInner {
                transport,
                account: Mutex::new(None),
                signer,
                context,
            }),
        }
    }

    /// Create a builder for [`GrpcClient`] connected to `url`
    pub fn builder() -> GrpcClientBuilder {
        GrpcClientBuilder::new()
    }

    // cosmos.auth

    /// Get auth params
    #[grpc_method(AuthQueryClient::params)]
    fn get_auth_params(&self) -> AsyncGrpcCall<AuthParams>;

    /// Get account
    #[grpc_method(AuthQueryClient::account)]
    fn get_account(&self, account: &AccAddress) -> AsyncGrpcCall<Account>;

    /// Get accounts
    #[grpc_method(AuthQueryClient::accounts)]
    fn get_accounts(&self) -> AsyncGrpcCall<Vec<Account>>;

    // cosmos.bank

    /// Retrieves the verified Celestia coin balance for the address.
    ///
    /// # Notes
    ///
    /// This returns the verified balance which is the one that was reported by
    /// the previous network block. In other words, if you transfer some coins,
    /// you need to wait 1 more block in order to see the new balance. If you want
    /// something more immediate then use [`GrpcClient::get_balance`].
    pub fn get_verified_balance(
        &self,
        address: &Address,
        header: &ExtendedHeader,
    ) -> AsyncGrpcCall<Coin> {
        let this = self.clone();
        let address = address.clone();
        let header = header.clone();

        AsyncGrpcCall::new(move |context| async move {
            this.get_verified_balance_impl(&address, &header, &context)
                .await
        })
    }

    /// Retrieves the Celestia coin balance for the given address.
    #[grpc_method(BankQueryClient::balance)]
    fn get_balance(&self, address: &Address, denom: impl Into<String>) -> AsyncGrpcCall<Coin>;

    /// Get balance of all coins
    #[grpc_method(BankQueryClient::all_balances)]
    fn get_all_balances(&self, address: &Address) -> AsyncGrpcCall<Vec<Coin>>;

    /// Get balance of all spendable coins
    #[grpc_method(BankQueryClient::spendable_balances)]
    fn get_spendable_balances(&self, address: &Address) -> AsyncGrpcCall<Vec<Coin>>;

    /// Get total supply
    #[grpc_method(BankQueryClient::total_supply)]
    fn get_total_supply(&self) -> AsyncGrpcCall<Vec<Coin>>;

    // cosmos.base.node

    /// Get node configuration
    #[grpc_method(ConfigServiceClient::config)]
    fn get_node_config(&self) -> AsyncGrpcCall<ConfigResponse>;

    // cosmos.base.tendermint

    /// Get latest block
    #[grpc_method(TendermintServiceClient::get_latest_block)]
    fn get_latest_block(&self) -> AsyncGrpcCall<Block>;

    /// Get block by height
    #[grpc_method(TendermintServiceClient::get_block_by_height)]
    fn get_block_by_height(&self, height: i64) -> AsyncGrpcCall<Block>;

    /// Issue a direct ABCI query to the application
    #[grpc_method(TendermintServiceClient::abci_query)]
    fn abci_query(
        &self,
        data: impl AsRef<[u8]>,
        path: impl Into<String>,
        height: u64,
        prove: bool,
    ) -> AsyncGrpcCall<AbciQueryResponse>;

    // cosmos.tx

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::broadcast_tx)]
    fn broadcast_tx(&self, tx_bytes: Vec<u8>, mode: BroadcastMode) -> AsyncGrpcCall<TxResponse>;

    /// Get Tx
    #[grpc_method(TxServiceClient::get_tx)]
    fn get_tx(&self, hash: Hash) -> AsyncGrpcCall<GetTxResponse>;

    /// Broadcast prepared and serialised transaction
    #[grpc_method(TxServiceClient::simulate)]
    fn simulate(&self, tx_bytes: Vec<u8>) -> AsyncGrpcCall<GasInfo>;

    // cosmos.staking

    /// Retrieves the delegation information between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::delegation)]
    fn query_delegation(
        &self,
        delegator_address: &AccAddress,
        validator_address: &ValAddress,
    ) -> AsyncGrpcCall<QueryDelegationResponse>;

    /// Retrieves the unbonding status between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::unbonding_delegation)]
    fn query_unbonding(
        &self,
        delegator_address: &AccAddress,
        validator_address: &ValAddress,
    ) -> AsyncGrpcCall<QueryUnbondingDelegationResponse>;

    /// Retrieves the status of the redelegations between a delegator and a validator
    // TODO: Expose this to JS and  UniFFI
    #[grpc_method(StakingQueryClient::redelegations)]
    fn query_redelegations(
        &self,
        delegator_address: &AccAddress,
        src_validator_address: &ValAddress,
        dest_validator_address: &ValAddress,
        pagination: Option<PageRequest>,
    ) -> AsyncGrpcCall<QueryRedelegationsResponse>;

    // celestia.blob

    /// Get blob params
    #[grpc_method(BlobQueryClient::params)]
    fn get_blob_params(&self) -> AsyncGrpcCall<BlobParams>;

    // celestia.core.tx

    /// Get status of the transaction
    #[grpc_method(TxStatusClient::tx_status)]
    fn tx_status(&self, hash: Hash) -> AsyncGrpcCall<TxStatusResponse>;

    // celestia.core.gas_estimation

    /// Estimate gas price for given transaction priority based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, return the network
    /// min gas price.
    #[grpc_method(GasEstimatorClient::estimate_gas_price)]
    fn estimate_gas_price(&self, priority: TxPriority) -> AsyncGrpcCall<f64>;

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
    fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> AsyncGrpcCall<GasEstimate>;

    /// Submit given message to celestia network.
    ///
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{GrpcClient, TxConfig};
    /// use celestia_proto::cosmos::bank::v1beta1::MsgSend;
    /// use celestia_types::state::{Address, Coin};
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let address = Address::from_account_veryfing_key(*signing_key.verifying_key());
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = GrpcClient::builder()
    ///     .url(grpc_url)
    ///     .signer_keypair(signing_key)
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
    pub fn submit_message<M>(&self, message: M, cfg: TxConfig) -> AsyncGrpcCall<TxInfo>
    where
        M: IntoProtobufAny + Send + 'static,
    {
        let this = self.clone();

        AsyncGrpcCall::new(move |context| async move {
            this.submit_message_impl(message, cfg, &context).await
        })
    }

    /// Submit given blobs to celestia network.
    ///
    /// # Example
    /// ```no_run
    /// # async fn docs() {
    /// use celestia_grpc::{GrpcClient, TxConfig};
    /// use celestia_types::state::{Address, Coin};
    /// use celestia_types::{AppVersion, Blob};
    /// use celestia_types::nmt::Namespace;
    /// use tendermint::crypto::default::ecdsa_secp256k1::SigningKey;
    ///
    /// let signing_key = SigningKey::random(&mut rand_core::OsRng);
    /// let address = Address::from_account_veryfing_key(*signing_key.verifying_key());
    /// let grpc_url = "public-celestia-mocha4-consensus.numia.xyz:9090";
    ///
    /// let tx_client = GrpcClient::builder()
    ///     .url(grpc_url)
    ///     .signer_keypair(signing_key)
    ///     .build()
    ///     .unwrap();
    ///
    /// let ns = Namespace::new_v0(b"abcd").unwrap();
    /// let blob = Blob::new(ns, "some data".into(), None, AppVersion::V3).unwrap();
    ///
    /// tx_client
    ///     .submit_blobs(&[blob], TxConfig::default())
    ///     .await
    ///     .unwrap();
    /// # }
    /// ```
    pub fn submit_blobs(&self, blobs: &[Blob], cfg: TxConfig) -> AsyncGrpcCall<TxInfo> {
        let this = self.clone();
        let blobs = blobs.to_vec();

        AsyncGrpcCall::new(move |context| async move {
            this.submit_blobs_impl(&blobs, cfg, &context).await
        })
    }

    /// Get client's app version
    ///
    /// Note that this function _may_ try to load the account information from the network and as
    /// such requires signed to be set up.
    pub fn app_version(&self) -> AsyncGrpcCall<AppVersion> {
        let this = self.clone();

        AsyncGrpcCall::new(move |context| async move {
            let (account, _) = this.load_account(&context).await?;
            Ok(account.app_version)
        })
    }

    /// Get client's chain id
    ///
    /// Note that this function _may_ try to load the account information from the network and as
    /// such requires signed to be set up.
    pub fn chain_id(&self) -> AsyncGrpcCall<Id> {
        let this = self.clone();

        AsyncGrpcCall::new(move |context| async move {
            let (account, _) = this.load_account(&context).await?;
            Ok(account.chain_id.clone())
        })
    }

    /// Get client's account public key if the signer is set
    pub fn get_account_pubkey(&self) -> Option<VerifyingKey> {
        self.inner.signer.as_ref().map(|config| config.pubkey)
    }
}

impl GrpcClient {
    async fn get_verified_balance_impl(
        &self,
        address: &Address,
        header: &ExtendedHeader,
        context: &Context,
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
            .context(context)
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

    async fn submit_message_impl<M>(
        &self,
        message: M,
        cfg: TxConfig,
        context: &Context,
    ) -> Result<TxInfo>
    where
        M: IntoProtobufAny,
    {
        let tx_body = RawTxBody {
            messages: vec![message.into_any()],
            memo: cfg.memo.clone().unwrap_or_default(),
            ..RawTxBody::default()
        };

        let (tx_hash, sequence) = self.sign_and_broadcast_tx(tx_body, cfg, context).await?;

        self.confirm_tx(tx_hash, sequence, context).await
    }

    async fn submit_blobs_impl(
        &self,
        blobs: &[Blob],
        cfg: TxConfig,
        context: &Context,
    ) -> Result<TxInfo> {
        if blobs.is_empty() {
            return Err(Error::TxEmptyBlobList);
        }
        let app_version = self.app_version().await?;
        for blob in blobs {
            blob.validate(app_version)?;
        }

        let (tx_hash, sequence) = self
            .sign_and_broadcast_blobs(blobs.to_vec(), cfg, context)
            .await?;

        self.confirm_tx(tx_hash, sequence, context).await
    }

    async fn load_account(
        &self,
        context: &Context,
    ) -> Result<(MappedMutexGuard<'_, AccountState>, &SignerConfig)> {
        let signer = self.inner.signer.as_ref().ok_or(Error::NoAccount)?;
        let mut account_guard = self.inner.account.lock().await;

        if account_guard.is_none() {
            let address = AccAddress::from(signer.pubkey);
            let account = self.get_account(&address).context(context).await?;

            let block = self.get_latest_block().context(context).await?;
            let app_version = block.header.version.app;
            let app_version = AppVersion::from_u64(app_version)
                .ok_or(celestia_types::Error::UnsupportedAppVersion(app_version))?;
            let chain_id = block.header.chain_id;

            *account_guard = Some(AccountState {
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
        context: &Context,
    ) -> Result<(u64, f64)> {
        let signer = self.inner.signer.as_ref().ok_or(Error::NoAccount)?;

        Ok(match (cfg.gas_limit, cfg.gas_price) {
            (Some(gas_limit), Some(gas_price)) => (gas_limit, gas_price),
            (Some(gas_limit), None) => {
                let gas_price = self
                    .estimate_gas_price(cfg.priority)
                    .context(context)
                    .await?;
                (gas_limit, gas_price)
            }
            (None, maybe_gas_price) => {
                let tx = sign_tx(
                    tx_body.clone(),
                    chain_id,
                    account,
                    &signer.pubkey,
                    &signer.signer,
                    0,
                    1,
                )
                .await?;

                let GasEstimate { price, usage } = self
                    .estimate_gas_price_and_usage(cfg.priority, tx.encode_to_vec())
                    .context(context)
                    .await?;
                (usage, maybe_gas_price.unwrap_or(price))
            }
        })
    }

    async fn sign_and_broadcast_blobs(
        &self,
        blobs: Vec<Blob>,
        cfg: TxConfig,
        context: &Context,
    ) -> Result<(Hash, u64)> {
        // lock the account; tx signing and broadcast must be atomic
        // because node requires all transactions to be sequenced by account.sequence
        let (account, signer) = self.load_account(context).await?;

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
                context,
            )
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil() as u64;
        let tx = sign_tx(
            pfb,
            account.chain_id.clone(),
            &account.account,
            &signer.pubkey,
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

        self.broadcast_tx_with_account(blob_tx.encode_to_vec(), cfg, account, context)
            .await
    }

    async fn sign_and_broadcast_tx(
        &self,
        tx: RawTxBody,
        cfg: TxConfig,
        context: &Context,
    ) -> Result<(Hash, u64)> {
        let (account, signer) = self.load_account(context).await?;

        let (gas_limit, gas_price) = self
            .calculate_transaction_gas_params(
                &tx,
                &cfg,
                account.chain_id.clone(),
                &account.account,
                context,
            )
            .await?;

        let fee = (gas_limit as f64 * gas_price).ceil();
        let tx = sign_tx(
            tx,
            account.chain_id.clone(),
            &account.account,
            &signer.pubkey,
            &signer.signer,
            gas_limit,
            fee as u64,
        )
        .await?;

        self.broadcast_tx_with_account(tx.encode_to_vec(), cfg, account, context)
            .await
    }

    async fn broadcast_tx_with_account(
        &self,
        tx: Vec<u8>,
        cfg: TxConfig,
        mut account: MappedMutexGuard<'_, AccountState>,
        context: &Context,
    ) -> Result<(Hash, u64)> {
        let resp = self
            .broadcast_tx(tx, BroadcastMode::Sync)
            .context(context)
            .await?;

        if resp.code != ErrorCode::Success {
            // if transaction failed due to insufficient fee, include info
            // whether gas price was estimated or explicitely set
            let message = if resp.code == ErrorCode::InsufficientFee {
                if cfg.gas_price.is_some() {
                    format!("Gas price was set via config. {}", resp.raw_log)
                } else {
                    format!("Gas price was estimated. {}", resp.raw_log)
                }
            } else {
                resp.raw_log
            };

            return Err(Error::TxBroadcastFailed(resp.txhash, resp.code, message));
        }

        let tx_sequence = account.account.sequence;
        account.account.sequence += 1;

        Ok((resp.txhash, tx_sequence))
    }

    async fn confirm_tx(&self, hash: Hash, sequence: u64, context: &Context) -> Result<TxInfo> {
        let mut interval = Interval::new(Duration::from_millis(500)).await;

        loop {
            let tx_status = self.tx_status(hash).context(context).await?;
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
                    let mut acc = self.inner.account.lock().await;
                    acc.as_mut().expect("account data present").account.sequence = sequence;
                    return Err(Error::TxEvicted(hash));
                }
                // this case should never happen for node that accepted a broadcast
                // however we handle it the same as evicted for extra safety
                TxStatus::Unknown => {
                    let mut acc = self.inner.account.lock().await;
                    acc.as_mut().expect("account data present").account.sequence = sequence;
                    return Err(Error::TxNotFound(hash));
                }
            }
        }
    }
}

impl fmt::Debug for GrpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GrpcClient { .. }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_utils::test_utils::async_test;

    #[async_test]
    async fn extending_client_context() {
        // TODO: test with simple grpc server?
        // let client = GrpcClient::builder()
        //     .url("http://foo")
        //     .metadata("test", "test")
        //     .build()
        //     .unwrap();
        // let call = client.app_version().metadata("test2", "test2").unwrap();

        // assert!(call.context.metadata.contains_key("test"));
        // assert!(call.context.metadata.contains_key("test2"));
    }
}
