use celestia_types::blob::RawBlob;
use celestia_types::state::{
    AccAddress, Address, Balance, QueryDelegationResponse, QueryRedelegationsResponse,
    QueryUnbondingDelegationResponse, RawTxResponse, Uint, ValAddress,
};
use jsonrpsee::proc_macros::rpc;

use crate::TxConfig;

#[rpc(client)]
pub trait State {
    /// AccountAddress retrieves the address of the node's account/signer
    #[method(name = "state.AccountAddress")]
    async fn state_account_address(&self) -> Result<Address, Error>;

    /// Balance retrieves the Celestia coin balance for the node's account/signer and verifies it against the corresponding block's AppHash.
    #[method(name = "state.Balance")]
    async fn state_balance(&self) -> Result<Balance, Error>;

    /// BalanceForAddress retrieves the Celestia coin balance for the given address and verifies the returned balance against the corresponding block's AppHash.
    ///
    /// # NOTE
    ///
    /// The balance returned is the balance reported by the block right before the node's current head (head-1). This is due to the fact that for block N, the block's `AppHash` is the result of applying the previous block's transaction list.
    #[method(name = "state.BalanceForAddress")]
    async fn state_balance_for_address(&self, addr: &Address) -> Result<Balance, Error>;

    /// BeginRedelegate sends a user's delegated tokens to a new validator for redelegation.
    #[method(name = "state.BeginRedelegate")]
    async fn state_begin_redelegate(
        &self,
        src: &ValAddress,
        dest: &ValAddress,
        amount: Uint,
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;

    /// CancelUnbondingDelegation cancels a user's pending undelegation from a validator.
    #[method(name = "state.CancelUnbondingDelegation")]
    async fn state_cancel_unbonding_delegation(
        &self,
        addr: &ValAddress,
        amount: Uint,
        height: Uint,
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;

    /// Delegate sends a user's liquid tokens to a validator for delegation.
    #[method(name = "state.Delegate")]
    async fn state_delegate(
        &self,
        addr: &ValAddress,
        amount: Uint,
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;

    /// IsStopped checks if the Module's context has been stopped.
    #[method(name = "state.IsStopped")]
    async fn state_is_stopped(&self) -> Result<bool, Error>;

    /// QueryDelegation retrieves the delegation information between a delegator and a validator.
    #[method(name = "state.QueryDelegation")]
    async fn state_query_delegation(
        &self,
        addr: &ValAddress,
    ) -> Result<QueryDelegationResponse, Error>;

    /// QueryRedelegations retrieves the status of the redelegations between a delegator and a validator.
    #[method(name = "state.QueryRedelegations")]
    async fn state_query_redelegations(
        &self,
        src: &ValAddress,
        dest: &ValAddress,
    ) -> Result<QueryRedelegationsResponse, Error>;

    /// QueryUnbonding retrieves the unbonding status between a delegator and a validator.
    #[method(name = "state.QueryUnbonding")]
    async fn state_query_unbonding(
        &self,
        addr: &ValAddress,
    ) -> Result<QueryUnbondingDelegationResponse, Error>;

    /// SubmitPayForBlob builds, signs and submits a PayForBlob transaction.
    #[method(name = "state.SubmitPayForBlob")]
    async fn state_submit_pay_for_blob(
        &self,
        blobs: &[RawBlob],
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;

    /// Transfer sends the given amount of coins from default wallet of the node to the given account address.
    #[method(name = "state.Transfer")]
    async fn state_transfer(
        &self,
        to: &AccAddress,
        amount: Uint,
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;

    /// Undelegate undelegates a user's delegated tokens, unbonding them from the current validator.
    #[method(name = "Undelegate")]
    async fn state_undelegate(
        &self,
        addr: &ValAddress,
        amount: Uint,
        config: TxConfig,
    ) -> Result<RawTxResponse, Error>;
}
