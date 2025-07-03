use std::sync::Arc;

use celestia_grpc::{DocSigner, IntoAny, TxClient, TxConfig, TxInfo};
use celestia_proto::cosmos::bank::v1beta1::MsgSend;
use celestia_rpc::blob::BlobsAtHeight;
use celestia_rpc::{
    BlobClient, Client as RpcClient, DasClient, HeaderClient, ShareClient, StateClient,
};
use celestia_types::nmt::{Namespace, NamespaceProof};
use celestia_types::state::{AccAddress, Address, Coin};
use celestia_types::Commitment;
use celestia_types::{AppVersion, Blob};
use jsonrpsee_core::client::Subscription;
use tendermint::chain::Id;
use tendermint::crypto::default::ecdsa_secp256k1::VerifyingKey;

use crate::{Context, Error, Result};

pub struct StateApi<S> {
    ctx: Arc<Context<S>>,
}

impl<S> StateApi<S>
where
    S: DocSigner,
{
    pub(crate) fn new(ctx: Arc<Context<S>>) -> StateApi<S> {
        StateApi { ctx }
    }

    /*
    // AccountAddress retrieves the address of the node's account/signer
    AccountAddress(ctx context.Context) (state.Address, error)

    // Balance retrieves the Celestia coin balance for the node's account/signer
    // and verifies it against the corresponding block's AppHash.
    Balance(ctx context.Context) (*state.Balance, error)

    // BalanceForAddress retrieves the Celestia coin balance for the given address and verifies
    // the returned balance against the corresponding block's AppHash.
    //
    // NOTE: the balance returned is the balance reported by the block right before
    // the node's current head (head-1). This is due to the fact that for block N, the block's
    // `AppHash` is the result of applying the previous block's transaction list.
    BalanceForAddress(ctx context.Context, addr state.Address) (*state.Balance, error)

    // Transfer sends the given amount of coins from default wallet of the node to the given account
    // address.
    // WRITE
    Transfer(
        ctx context.Context, to state.AccAddress, amount state.Int, config *state.TxConfig,
    ) (*state.TxResponse, error)

    // SubmitPayForBlob builds, signs and submits a PayForBlob transaction.
    // WRITE
    SubmitPayForBlob(
        ctx context.Context,
        blobs []*libshare.Blob,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // CancelUnbondingDelegation cancels a user's pending undelegation from a validator.
    // WRITE
    CancelUnbondingDelegation(
        ctx context.Context,
        valAddr state.ValAddress,
        amount,
        height state.Int,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // BeginRedelegate sends a user's delegated tokens to a new validator for redelegation.
    // WRITE
    BeginRedelegate(
        ctx context.Context,
        srcValAddr,
        dstValAddr state.ValAddress,
        amount state.Int,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // Undelegate undelegates a user's delegated tokens, unbonding them from the current validator.
    // WRITE
    Undelegate(
        ctx context.Context,
        delAddr state.ValAddress,
        amount state.Int,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // Delegate sends a user's liquid tokens to a validator for delegation.
    // WRITE
    Delegate(
        ctx context.Context,
        delAddr state.ValAddress,
        amount state.Int,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // QueryDelegation retrieves the delegation information between a delegator and a validator.
    QueryDelegation(ctx context.Context, valAddr state.ValAddress) (*types.QueryDelegationResponse, error)

    // QueryUnbonding retrieves the unbonding status between a delegator and a validator.
    QueryUnbonding(ctx context.Context, valAddr state.ValAddress) (*types.QueryUnbondingDelegationResponse, error)

    // QueryRedelegations retrieves the status of the redelegations between a delegator and a validator.
    QueryRedelegations(
        ctx context.Context,
        srcValAddr,
        dstValAddr state.ValAddress,
    ) (*types.QueryRedelegationsResponse, error)

    // WRITE
    GrantFee(
        ctx context.Context,
        grantee state.AccAddress,
        amount state.Int,
        config *state.TxConfig,
    ) (*state.TxResponse, error)

    // WRITE
    RevokeGrantFee(
        ctx context.Context,
        grantee state.AccAddress,
        config *state.TxConfig,
    ) (*state.TxResponse, error)
    */

    // TODO: https://docs.rs/celestia-grpc/latest/celestia_grpc/grpc/struct.GrpcClient.html

    pub fn account_address(&self) -> Result<AccAddress> {
        todo!();
    }

    pub async fn balance(&self) -> Result<Coin> {
        let address = self.account_address()?;
        self.balance_for_address(address).await
    }

    pub async fn balance_for_address(&self, address: AccAddress) -> Result<Coin> {
        let address = address.into();

        match self.ctx.grpc() {
            Ok(grpc) => Ok(grpc.get_balance(&address, "utia").await?),
            Err(_) => Ok(self.ctx.rpc.state_balance_for_address(&address).await?),
        }
    }

    pub async fn submit_message<M>(&self, message: M, cfg: TxConfig) -> Result<TxInfo>
    where
        M: IntoAny,
    {
        Ok(self.ctx.grpc()?.submit_message(message, cfg).await?)
    }

    pub async fn transfer(
        &self,
        to_address: AccAddress,
        amount: Coin,
        cfg: TxConfig,
    ) -> Result<TxInfo> {
        let from_address = self.account_address()?;

        let msg = MsgSend {
            from_address: from_address.to_string(),
            to_address: to_address.to_string(),
            amount: vec![amount.into()],
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

    pub async fn cancel_unbonding_delegation(&self) -> Result<()> {
        todo!();
    }

    pub async fn begin_redelegate(&self) -> Result<()> {
        todo!();
    }

    pub async fn undelegate(&self) -> Result<()> {
        todo!();
    }

    pub async fn delegate(&self) -> Result<()> {
        todo!();
    }

    pub async fn query_delegation(&self) -> Result<()> {
        todo!();
    }

    pub async fn query_unbonding(&self) -> Result<()> {
        todo!();
    }

    pub async fn query_redelegations(&self) -> Result<()> {
        todo!();
    }

    pub async fn grant_fee(&self) -> Result<()> {
        todo!();
    }

    pub async fn revoke_grant_fee(&self) -> Result<()> {
        todo!();
    }
}
