use std::sync::Arc;

use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_proto::cosmos::staking::v1beta1::{
    MsgBeginRedelegate, MsgCancelUnbondingDelegation, MsgDelegate, MsgUndelegate,
};
use celestia_rpc::{HeaderClient, StateClient};
use celestia_types::state::{
    AccAddress, Address, Coin, PageRequest, QueryDelegationResponse, QueryRedelegationsResponse,
    QueryUnbondingDelegationResponse, ValAddress,
};
use celestia_types::Blob;
use k256::ecdsa::VerifyingKey;

use crate::client::Context;
use crate::tx::{GasEstimate, IntoProtobufAny, TxConfig, TxInfo, TxPriority};
use crate::utils::height_i64;
use crate::Result;

/// State API for quering and submiting TXs to a consensus node.
pub struct StateApi {
    ctx: Arc<Context>,
}

impl StateApi {
    pub(crate) fn new(ctx: Arc<Context>) -> StateApi {
        StateApi { ctx }
    }

    /// Returns the public key of the signer.
    pub fn pubkey(&self) -> Result<VerifyingKey> {
        self.ctx.pubkey().cloned()
    }

    /// Returns the address of signer.
    pub fn account_address(&self) -> Result<AccAddress> {
        let pubkey = self.ctx.pubkey()?.to_owned();
        Ok(AccAddress::new(pubkey.into()))
    }

    /// Retrieves the Celestia coin balance for the signer.
    ///
    /// # Notes
    ///
    /// This returns the verified balance which is the one that was reported by
    /// the previous network block. In other words, if you transfer some coins,
    /// you need to wait 1 more block in order to see the new balance. If you want
    /// something more immediate then use [`StateApi::balance_unverified`].
    pub async fn balance(&self) -> Result<u64> {
        let address = self.account_address()?;
        self.balance_for_address(&address).await
    }

    /// Retrieves the Celestia coin balance for the signer.
    pub async fn balance_unverified(&self) -> Result<u64> {
        let address = self.account_address()?;
        self.balance_for_address_unverified(&address).await
    }

    /// Retrieves the Celestia coin balance for the given address.
    ///
    /// # Notes
    ///
    /// This returns the verified balance which is the one that was reported by
    /// the previous network block. In other words, if you transfer some coins,
    /// you need to wait 1 more block in order to see the new balance. If you want
    /// something more immediate then use [`StateApi::balance_for_address_unverified`].
    ///
    /// This is the only method of [`StateApi`] that fallbacks to RPC endpoint
    /// when gRPC endpoint wasn't set.
    pub async fn balance_for_address(&self, address: &AccAddress) -> Result<u64> {
        let address = Address::AccAddress(address.to_owned());

        let grpc = match self.ctx.grpc() {
            Ok(grpc) => grpc,
            Err(_) => {
                return Ok(self
                    .ctx
                    .rpc
                    .state_balance_for_address(&address)
                    .await?
                    .amount());
            }
        };

        let head = self.ctx.rpc.header_network_head().await?;
        head.validate()?;

        Ok(grpc.get_verified_balance(&address, &head).await?.amount())
    }

    /// Retrieves the Celestia coin balance for the given address.
    pub async fn balance_for_address_unverified(&self, address: &AccAddress) -> Result<u64> {
        Ok(self
            .ctx
            .grpc()?
            .get_balance(&address.to_owned().into(), "utia")
            .await?
            .amount())
    }

    /// Estimate gas price for given transaction priority based
    /// on the gas prices of the transactions in the last five blocks.
    ///
    /// If no transaction is found in the last five blocks, it returns the
    /// network min gas price.
    pub async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64> {
        Ok(self.ctx.grpc()?.estimate_gas_price(priority).await?)
    }

    /// Estimate gas price for transaction with given priority and estimate gas usage
    /// for provided serialised transaction.
    ///
    /// The gas price estimation is based on the gas prices of the transactions
    /// in the last five blocks. If no transaction is found in the last five blocks,
    /// it returns the network min gas price.
    ///
    /// The gas used is estimated using the state machine simulation.
    async fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> Result<GasEstimate> {
        Ok(self
            .ctx
            .grpc()?
            .estimate_gas_price_and_usage(priority, tx_bytes)
            .await?)
    }

    /// Submit given message to celestia network.
    ///
    /// # Example
    /// ```no_run
    /// # use celestia_client::{Client, Result};
    /// # use celestia_client::tx::TxConfig;
    /// # const RPC_URL: &str = "http://localhost:26658";
    /// # const GRPC_URL : &str = "http://localhost:19090";
    /// # async fn docs() -> Result<()> {
    /// use celestia_proto::cosmos::bank::v1beta1::MsgSend;
    /// use celestia_types::state::{Address, Coin};
    ///
    /// let client = Client::builder()
    ///     .rpc_url(RPC_URL)
    ///     .grpc_url(GRPC_URL)
    ///     .private_key_hex("...")
    ///     .build()
    ///     .await?;
    ///
    /// let msg = MsgSend {
    ///     from_address: client.state().account_address()?.to_string(),
    ///     to_address: "celestia169s50psyj2f4la9a2235329xz7rk6c53zhw9mm".to_string(),
    ///     amount: vec![Coin::utia(12345).into()],
    /// };
    ///
    /// client
    ///     .state()
    ///     .submit_message(msg, TxConfig::default())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn submit_message<M>(&self, message: M, cfg: TxConfig) -> Result<TxInfo>
    where
        M: IntoProtobufAny,
    {
        Ok(self.ctx.grpc()?.submit_message(message, cfg).await?)
    }

    /// Sends the given amount of coins from signer's wallet to the given account address.
    pub async fn transfer(
        &self,
        to_address: &AccAddress,
        amount: u64,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let from_address = self.account_address()?;

        let msg = MsgSend {
            from_address: from_address.to_string(),
            to_address: to_address.to_string(),
            amount: vec![Coin::utia(amount).into()],
        };

        self.submit_message(msg, cfg).await
    }

    /// Builds, signs and submits a PayForBlob transaction.
    ///
    /// # Note
    ///
    /// This is the same as [`BlobApi::submit`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use celestia_client::{Client, Result};
    /// # use celestia_client::tx::TxConfig;
    /// # const RPC_URL: &str = "http://localhost:26658";
    /// # const GRPC_URL : &str = "http://localhost:19090";
    /// # async fn docs() -> Result<()> {
    /// use celestia_types::nmt::Namespace;
    /// use celestia_types::state::{Address, Coin};
    /// use celestia_types::{AppVersion, Blob};
    ///
    /// let client = Client::builder()
    ///     .rpc_url(RPC_URL)
    ///     .grpc_url(GRPC_URL)
    ///     .private_key_hex("...")
    ///     .build()
    ///     .await?;
    ///
    /// let ns = Namespace::new_v0(b"abcd").unwrap();
    /// let blob = Blob::new(ns, "some data".into(), AppVersion::V3).unwrap();
    ///
    /// client
    ///     .state()
    ///     .submit_pay_for_blob(&[blob], TxConfig::default())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`BlobApi::submit`]: crate::api::BlobApi::submit
    pub async fn submit_pay_for_blob(&self, blobs: &[Blob], cfg: TxConfig) -> Result<TxInfo> {
        Ok(self.ctx.grpc()?.submit_blobs(blobs, cfg).await?)
    }

    /// Cancels signer's pending undelegation from a validator.
    pub async fn cancel_unbonding_delegation(
        &self,
        validator_address: &ValAddress,
        amount: u64,
        creation_height: u64,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let delegator_address = self.account_address()?;

        let msg = MsgCancelUnbondingDelegation {
            delegator_address: delegator_address.to_string(),
            validator_address: validator_address.to_string(),
            amount: Some(Coin::utia(amount).into()),
            creation_height: height_i64(creation_height)?,
        };

        self.submit_message(msg, cfg).await
    }

    /// Sends signer's delegated tokens to a new validator for redelegation.
    pub async fn begin_redelegate(
        &self,
        src_validator_address: &ValAddress,
        dest_validator_address: &ValAddress,
        amount: u64,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let delegator_address = self.account_address()?;

        let msg = MsgBeginRedelegate {
            delegator_address: delegator_address.to_string(),
            validator_src_address: src_validator_address.to_string(),
            validator_dst_address: dest_validator_address.to_string(),
            amount: Some(Coin::utia(amount).into()),
        };

        self.submit_message(msg, cfg).await
    }

    /// Undelegates signer's delegated tokens, unbonding them from the current validator.
    pub async fn undelegate(
        &self,
        validator_address: &ValAddress,
        amount: u64,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let delegator_address = self.account_address()?;

        let msg = MsgUndelegate {
            delegator_address: delegator_address.to_string(),
            validator_address: validator_address.to_string(),
            amount: Some(Coin::utia(amount).into()),
        };

        self.submit_message(msg, cfg).await
    }

    /// Sends signer's liquid tokens to a validator for delegation.
    pub async fn delegate(
        &self,
        validator_address: &ValAddress,
        amount: u64,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let delegator_address = self.account_address()?;

        let msg = MsgDelegate {
            delegator_address: delegator_address.to_string(),
            validator_address: validator_address.to_string(),
            amount: Some(Coin::utia(amount).into()),
        };

        self.submit_message(msg, cfg).await
    }

    /// Retrieves the delegation information between signer and a validator.
    pub async fn query_delegation(
        &self,
        validator_address: &ValAddress,
    ) -> Result<QueryDelegationResponse> {
        let delegator_address = self.account_address()?;

        let resp = self
            .ctx
            .grpc()?
            .query_delegation(&delegator_address, validator_address)
            .await?;

        Ok(resp)
    }

    /// Retrieves the unbonding status between signer and a validator.
    pub async fn query_unbonding(
        &self,
        validator_address: &ValAddress,
    ) -> Result<QueryUnbondingDelegationResponse> {
        let delegator_address = self.account_address()?;

        let resp = self
            .ctx
            .grpc()?
            .query_unbonding(&delegator_address, validator_address)
            .await?;

        Ok(resp)
    }

    /// Retrieves the status of the redelegations between signer and a validator.
    pub async fn query_redelegations(
        &self,
        src_validator_address: &ValAddress,
        dest_validator_address: &ValAddress,
    ) -> Result<QueryRedelegationsResponse> {
        let delegator_address = self.account_address()?;

        let mut full_resp = QueryRedelegationsResponse {
            responses: Vec::new(),
            pagination: None,
        };

        let mut next_key = Vec::new();

        loop {
            let mut resp = self
                .ctx
                .grpc()?
                .query_redelegations(
                    &delegator_address,
                    src_validator_address,
                    dest_validator_address,
                    Some(PageRequest {
                        key: next_key,
                        ..Default::default()
                    }),
                )
                .await?;

            full_resp.responses.append(&mut resp.responses);

            match resp.pagination {
                Some(pagination) => next_key = pagination.next_key,
                None => break,
            }
        }

        Ok(full_resp)
    }
}

#[cfg(test)]
mod tests {
    use celestia_grpc::TxConfig;
    use k256::ecdsa::SigningKey;
    use lumina_utils::test_utils::async_test;

    use crate::test_utils::{
        new_client, new_client_random_account, new_read_only_client, node0_address,
        validator_address,
    };
    use crate::Error;

    #[async_test]
    async fn transfer() {
        let (_lock, client) = new_client().await;

        let random_key = SigningKey::random(&mut rand::thread_rng());
        let random_acc = random_key.verifying_key().into();

        client
            .state()
            .transfer(&random_acc, 123, TxConfig::default())
            .await
            .unwrap();

        assert_eq!(
            client
                .state()
                .balance_for_address_unverified(&random_acc)
                .await
                .unwrap(),
            123
        );
    }

    #[async_test]
    async fn delegation() {
        let client = new_client_random_account().await;
        let validator_addr = validator_address();
        let client_addr = client.state().account_address().unwrap();

        // Test delegation
        client
            .state()
            .delegate(&validator_addr, 100, TxConfig::default())
            .await
            .unwrap();

        let del = client
            .state()
            .query_delegation(&validator_addr)
            .await
            .unwrap();

        assert_eq!(del.response.balance, 100);
        assert_eq!(del.response.delegation.delegator_address, client_addr);
        assert_eq!(del.response.delegation.validator_address, validator_addr);
        assert_eq!(del.response.delegation.shares, 100.into());

        // Test unbonding
        let unbond_tx_height = client
            .state()
            .undelegate(&validator_addr, 10, TxConfig::default())
            .await
            .unwrap()
            .height
            .value();

        let unbond = client
            .state()
            .query_unbonding(&validator_addr)
            .await
            .unwrap();

        assert_eq!(unbond.unbond.delegator_address, client_addr);
        assert_eq!(unbond.unbond.validator_address, validator_addr);
        assert_eq!(unbond.unbond.entries.len(), 1);
        assert_eq!(
            unbond.unbond.entries[0].creation_height.value(),
            unbond_tx_height
        );
        assert_eq!(unbond.unbond.entries[0].initial_balance, 10);
        assert_eq!(unbond.unbond.entries[0].balance, 10);

        let del = client
            .state()
            .query_delegation(&validator_addr)
            .await
            .unwrap();

        assert_eq!(del.response.balance, 90);
        assert_eq!(del.response.delegation.delegator_address, client_addr);
        assert_eq!(del.response.delegation.validator_address, validator_addr);
        assert_eq!(del.response.delegation.shares, 90.into());

        // Test partial cancel unbonding
        client
            .state()
            .cancel_unbonding_delegation(&validator_addr, 3, unbond_tx_height, TxConfig::default())
            .await
            .unwrap();

        let unbond = client
            .state()
            .query_unbonding(&validator_addr)
            .await
            .unwrap();

        assert_eq!(unbond.unbond.delegator_address, client_addr);
        assert_eq!(unbond.unbond.validator_address, validator_addr);
        assert_eq!(unbond.unbond.entries.len(), 1);
        assert_eq!(
            unbond.unbond.entries[0].creation_height.value(),
            unbond_tx_height
        );
        assert_eq!(unbond.unbond.entries[0].initial_balance, 7);
        assert_eq!(unbond.unbond.entries[0].balance, 7);

        let del = client
            .state()
            .query_delegation(&validator_addr)
            .await
            .unwrap();

        assert_eq!(del.response.balance, 93);
        assert_eq!(del.response.delegation.delegator_address, client_addr);
        assert_eq!(del.response.delegation.validator_address, validator_addr);
        assert_eq!(del.response.delegation.shares, 93.into());

        // Test fully cancel unbonding
        client
            .state()
            .cancel_unbonding_delegation(&validator_addr, 7, unbond_tx_height, TxConfig::default())
            .await
            .unwrap();

        let err = client
            .state()
            .query_unbonding(&validator_addr)
            .await
            .unwrap_err();

        assert_eq!(err.as_grpc_status().unwrap().code(), tonic::Code::NotFound);

        let del = client
            .state()
            .query_delegation(&validator_addr)
            .await
            .unwrap();

        assert_eq!(del.response.balance, 100);
        assert_eq!(del.response.delegation.delegator_address, client_addr);
        assert_eq!(del.response.delegation.validator_address, validator_addr);
        assert_eq!(del.response.delegation.shares, 100.into());
    }

    #[async_test]
    async fn balance_for_address() {
        let client_ro = new_read_only_client().await;

        // Read only mode allows calling `balance_for_address`
        let addr = node0_address();
        let balance = client_ro.state().balance_for_address(&addr).await.unwrap();
        assert!(balance > 0);

        // Read only mode does not allow calling `balance_for_address_unverified`.
        let e = client_ro
            .state()
            .balance_for_address_unverified(&addr)
            .await
            .unwrap_err();
        assert!(matches!(e, Error::ReadOnlyMode));

        // Read only mode does not allow calling `balance`
        let e = client_ro.state().balance().await.unwrap_err();
        assert!(matches!(e, Error::ReadOnlyMode));

        // Read only mode does not allow calling `balance_unverified`
        let e = client_ro.state().balance().await.unwrap_err();
        assert!(matches!(e, Error::ReadOnlyMode));
    }
}
