use celestia_types::state::{Address, Balance};
use jsonrpsee::proc_macros::rpc;

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
}
