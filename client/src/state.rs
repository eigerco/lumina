use std::sync::Arc;

use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_proto::cosmos::staking::v1beta1::{
    MsgBeginRedelegate, MsgCancelUnbondingDelegation, MsgDelegate, MsgUndelegate,
};
use celestia_rpc::StateClient;
use celestia_types::state::{
    AccAddress, Address, Coin, PageRequest, QueryDelegationResponse, QueryRedelegationsResponse,
    QueryUnbondingDelegationResponse, ValAddress,
};
use celestia_types::Blob;

use crate::client::Context;
use crate::tx::{IntoAny, TxConfig, TxInfo};
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

    /// Returns the address of signer.
    pub fn account_address(&self) -> Result<AccAddress> {
        let pubkey = self.ctx.pubkey()?.to_owned();
        Ok(AccAddress::new(pubkey.into()))
    }

    /// Retrieves the Celestia coin balance for the signer.
    pub async fn balance(&self) -> Result<u64> {
        let address = self.account_address()?;
        self.balance_for_address(address).await
    }

    /// Retrieves the Celestia coin balance for the given address.
    ///
    /// # Notes
    ///
    /// This is the only method of [`StateApi`] that fallbacks to bridge node
    /// if consensus node is not set in [`Client`].
    pub async fn balance_for_address(&self, address: AccAddress) -> Result<u64> {
        let address = Address::AccAddress(address);

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

        // TODO: Verify balance with AbciQuery is ready.
        let coin = grpc.get_balance(&address, "utia").await?;

        Ok(coin.amount())
    }

    /// Submit given message to celestia network.
    ///
    /// When no gas price is specified through config, it will automatically
    /// handle updating client's gas price when consensus updates minimal
    /// gas price.
    ///
    /// # Example
    /// ```no_run
    /// // TODO
    /// ```
    pub async fn submit_message<M>(&self, message: M, cfg: TxConfig) -> Result<TxInfo>
    where
        M: IntoAny,
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
    /// When no gas price is specified through config, it will automatically
    /// handle updating client's gas price when consensus updates minimal
    /// gas price.
    ///
    /// # Example
    /// ```no_run
    /// TODO
    /// ```
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
            redelegation_responses: Vec::new(),
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

            full_resp
                .redelegation_responses
                .append(&mut resp.redelegation_responses);

            match resp.pagination {
                Some(pagination) => next_key = pagination.next_key,
                None => break,
            }
        }

        Ok(full_resp)
    }

    pub async fn grant_fee(&self) -> Result<()> {
        todo!();
    }

    pub async fn revoke_grant_fee(&self) -> Result<()> {
        todo!();
    }
}
