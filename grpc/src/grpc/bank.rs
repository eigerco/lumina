use celestia_proto::cosmos::bank::v1beta1::{
    QueryAllBalancesRequest, QueryAllBalancesResponse, QueryBalanceRequest, QueryBalanceResponse,
    QuerySpendableBalancesRequest, QuerySpendableBalancesResponse, QueryTotalSupplyRequest,
    QueryTotalSupplyResponse,
};
use celestia_types::state::{Address, Coin};

use crate::grpc::{FromGrpcResponse, IntoGrpcParam};
use crate::{Error, Result};

impl<I> IntoGrpcParam<QueryBalanceRequest> for (&Address, I)
where
    I: Into<String>,
{
    fn into_parameter(self) -> QueryBalanceRequest {
        QueryBalanceRequest {
            address: self.0.to_string(),
            denom: self.1.into(),
        }
    }
}

impl FromGrpcResponse<Coin> for QueryBalanceResponse {
    fn try_from_response(self) -> Result<Coin> {
        Ok(self
            .balance
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?)
    }
}

impl IntoGrpcParam<QueryAllBalancesRequest> for &Address {
    fn into_parameter(self) -> QueryAllBalancesRequest {
        QueryAllBalancesRequest {
            address: self.to_string(),
            pagination: None,
        }
    }
}

impl FromGrpcResponse<Vec<Coin>> for QueryAllBalancesResponse {
    fn try_from_response(self) -> Result<Vec<Coin>> {
        Ok(self
            .balances
            .into_iter()
            .map(|coin| coin.try_into())
            .collect::<Result<_, _>>()?)
    }
}

impl IntoGrpcParam<QuerySpendableBalancesRequest> for &Address {
    fn into_parameter(self) -> QuerySpendableBalancesRequest {
        QuerySpendableBalancesRequest {
            address: self.to_string(),
            pagination: None,
        }
    }
}

impl FromGrpcResponse<Vec<Coin>> for QuerySpendableBalancesResponse {
    fn try_from_response(self) -> Result<Vec<Coin>> {
        Ok(self
            .balances
            .into_iter()
            .map(|coin| coin.try_into())
            .collect::<Result<_, _>>()?)
    }
}

impl IntoGrpcParam<QueryTotalSupplyRequest> for () {
    fn into_parameter(self) -> QueryTotalSupplyRequest {
        QueryTotalSupplyRequest { pagination: None }
    }
}

impl FromGrpcResponse<Vec<Coin>> for QueryTotalSupplyResponse {
    fn try_from_response(self) -> Result<Vec<Coin>> {
        Ok(self
            .supply
            .into_iter()
            .map(|coin| coin.try_into())
            .collect::<Result<_, _>>()?)
    }
}
