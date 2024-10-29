use prost::{Message, Name};

use celestia_proto::celestia::blob::v1::QueryParamsResponse as QueryBlobParamsResponse;
use celestia_proto::cosmos::auth::v1beta1::QueryParamsResponse as QueryAuthParamsResponse;
use celestia_proto::cosmos::auth::v1beta1::{BaseAccount as RawBaseAccount, QueryAccountResponse, QueryAccountRequest};
use celestia_proto::cosmos::base::node::v1beta1::ConfigResponse;
use celestia_proto::cosmos::base::tendermint::v1beta1::{
    GetBlockByHeightResponse, GetLatestBlockResponse,
    GetBlockByHeightRequest
};

use celestia_tendermint::block::Block as TendermintBlock;

use crate::tonic::Error;

pub struct GasPrice(pub f64);
pub struct Block(pub TendermintBlock);
#[derive(Debug)]
pub struct BlobParams {
    pub gas_per_blob_byte: u32,
    pub gov_max_square_size: u64,
}
#[derive(Debug)]
pub struct AuthParams {
    pub max_memo_characters: u64,
    pub tx_sig_limit: u64,
    pub tx_size_cost_per_byte: u64,
    pub sig_verify_cost_ed25519: u64,
    pub sig_verify_cost_secp256k1: u64,
}
#[derive(Debug)]
pub struct BaseAccount {
    pub address: String,
    //pub pub_key
    account_number: u64,
    sequence: u64,
}

pub(crate) trait IntoGrpcParam<T> {
    fn into_parameter(self) -> T;
}

impl IntoGrpcParam<GetBlockByHeightRequest> for i64 {
    fn into_parameter(self) -> GetBlockByHeightRequest {
        GetBlockByHeightRequest {
            height: self
        }
    }
}

impl IntoGrpcParam<QueryAccountRequest> for String {
    fn into_parameter(self) -> QueryAccountRequest {
        QueryAccountRequest {
            address: self
        }
    }
}

impl TryFrom<ConfigResponse> for GasPrice {
    type Error = Error;

    fn try_from(item: ConfigResponse) -> Result<Self, Self::Error> {
        println!("II: {item:#?}");
        const UNITS_SUFFIX: &str = "utia";

        let min_gas_price_with_suffix = item.minimum_gas_price;
        let min_gas_price_str = min_gas_price_with_suffix
            .strip_suffix(UNITS_SUFFIX)
            .ok_or(Error::FailedToParseResponse)?;
        let min_gas_price = min_gas_price_str
            .parse::<f64>()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(GasPrice(min_gas_price))
    }
}

impl TryFrom<GetBlockByHeightResponse> for Block {
    type Error = Error;

    fn try_from(item: GetBlockByHeightResponse) -> Result<Self, Self::Error> {
        Ok(Block(
            item.block.ok_or(Error::FailedToParseResponse)?.try_into()?,
        ))
    }
}

impl TryFrom<GetLatestBlockResponse> for Block {
    type Error = Error;

    fn try_from(item: GetLatestBlockResponse) -> Result<Self, Self::Error> {
        Ok(Block(
            item.block.ok_or(Error::FailedToParseResponse)?.try_into()?,
        ))
    }
}

impl TryFrom<QueryBlobParamsResponse> for BlobParams {
    type Error = Error;

    fn try_from(item: QueryBlobParamsResponse) -> Result<Self, Self::Error> {
        let params = item.params.ok_or(Error::FailedToParseResponse)?;
        Ok(BlobParams {
            gas_per_blob_byte: params.gas_per_blob_byte,
            gov_max_square_size: params.gov_max_square_size,
        })
    }
}

impl TryFrom<QueryAuthParamsResponse> for AuthParams {
    type Error = Error;

    fn try_from(item: QueryAuthParamsResponse) -> Result<Self, Self::Error> {
        let params = item.params.ok_or(Error::FailedToParseResponse)?;
        Ok(AuthParams {
            max_memo_characters: params.max_memo_characters,
            tx_sig_limit: params.tx_sig_limit,
            tx_size_cost_per_byte: params.tx_size_cost_per_byte,
            sig_verify_cost_ed25519: params.sig_verify_cost_ed25519,
            sig_verify_cost_secp256k1: params.sig_verify_cost_secp256k1,
        })
    }
}

impl TryFrom<QueryAccountResponse> for BaseAccount {
    type Error = Error;

    fn try_from(item: QueryAccountResponse) -> Result<Self, Self::Error> {
        let account = item.account.ok_or(Error::FailedToParseResponse)?;
        if account.type_url != RawBaseAccount::type_url() {
            return Err(Error::UnexpectedResponseType(account.type_url));
        }
        let base_account =
            RawBaseAccount::decode(&*account.value).map_err(|_| Error::FailedToParseResponse)?;

        Ok(Self {
            address: base_account.address,
            account_number: base_account.account_number,
            sequence: base_account.sequence,
        })
    }
}
