use prost::{Message, Name};

use celestia_proto::cosmos::auth::v1beta1::{
    BaseAccount as RawBaseAccount, ModuleAccount as RawModuleAccount, QueryAccountRequest,
    QueryAccountResponse, QueryAccountsRequest, QueryAccountsResponse,
    QueryParamsRequest as QueryAuthParamsRequest, QueryParamsResponse as QueryAuthParamsResponse,
};
use celestia_types::auth::{AuthParams, BaseAccount, ModuleAccount};

use crate::types::make_empty_params;
use crate::types::{FromGrpcResponse, IntoGrpcParam};
use crate::Error;

/// Enum representing different types of account
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum Account {
    /// Base account type
    Base(BaseAccount),
    /// Account for modules that holds coins on a pool
    Module(ModuleAccount),
}

impl FromGrpcResponse<AuthParams> for QueryAuthParamsResponse {
    fn try_from_response(self) -> Result<AuthParams, Error> {
        let params = self.params.ok_or(Error::FailedToParseResponse)?;
        Ok(AuthParams {
            max_memo_characters: params.max_memo_characters,
            tx_sig_limit: params.tx_sig_limit,
            tx_size_cost_per_byte: params.tx_size_cost_per_byte,
            sig_verify_cost_ed25519: params.sig_verify_cost_ed25519,
            sig_verify_cost_secp256k1: params.sig_verify_cost_secp256k1,
        })
    }
}

impl FromGrpcResponse<Account> for QueryAccountResponse {
    fn try_from_response(self) -> Result<Account, Error> {
        account_from_any(self.account.ok_or(Error::FailedToParseResponse)?)
    }
}

impl FromGrpcResponse<Vec<Account>> for QueryAccountsResponse {
    fn try_from_response(self) -> Result<Vec<Account>, Error> {
        self.accounts.into_iter().map(account_from_any).collect()
    }
}

make_empty_params!(QueryAuthParamsRequest);

impl IntoGrpcParam<QueryAccountRequest> for String {
    fn into_parameter(self) -> QueryAccountRequest {
        QueryAccountRequest { address: self }
    }
}

impl IntoGrpcParam<QueryAccountsRequest> for () {
    fn into_parameter(self) -> QueryAccountsRequest {
        QueryAccountsRequest { pagination: None }
    }
}

fn account_from_any(any: pbjson_types::Any) -> Result<Account, Error> {
    let account = if any.type_url == RawBaseAccount::type_url() {
        let base_account =
            RawBaseAccount::decode(&*any.value).map_err(|_| Error::FailedToParseResponse)?;
        Account::Base(base_account.try_into()?)
    } else if any.type_url == RawModuleAccount::type_url() {
        let module_account =
            RawModuleAccount::decode(&*any.value).map_err(|_| Error::FailedToParseResponse)?;
        Account::Module(module_account.try_into()?)
    } else {
        return Err(Error::UnexpectedResponseType(any.type_url));
    };

    Ok(account)
}
