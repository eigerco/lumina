use prost::{Message, Name};

use celestia_types::auth::{AuthParams, BaseAccount};
use celestia_types::blob::BlobParams;

use celestia_proto::celestia::blob::v1::QueryParamsResponse as QueryBlobParamsResponse;
use celestia_proto::cosmos::auth::v1beta1::QueryParamsResponse as QueryAuthParamsResponse;
use celestia_proto::cosmos::auth::v1beta1::{
    BaseAccount as RawBaseAccount, QueryAccountRequest, QueryAccountResponse,
};
use celestia_proto::cosmos::base::node::v1beta1::ConfigResponse;
use celestia_proto::cosmos::base::tendermint::v1beta1::{
    GetBlockByHeightRequest, GetBlockByHeightResponse, GetLatestBlockResponse,
};

use celestia_tendermint::block::Block as TendermintBlock;

use cosmrs::crypto::PublicKey;
use cosmrs::Any;

use crate::tonic::Error;

pub struct Block(pub TendermintBlock);

pub(crate) trait FromGrpcResponse<T> {
    fn try_from_response(self) -> Result<T, Error>;
}

pub(crate) trait IntoGrpcParam<T> {
    fn into_parameter(self) -> T;
}

impl FromGrpcResponse<BlobParams> for QueryBlobParamsResponse {
    fn try_from_response(self) -> Result<BlobParams, Error> {
        let params = self.params.ok_or(Error::FailedToParseResponse)?;
        Ok(BlobParams {
            gas_per_blob_byte: params.gas_per_blob_byte,
            gov_max_square_size: params.gov_max_square_size,
        })
    }
}

impl FromGrpcResponse<Block> for GetBlockByHeightResponse {
    fn try_from_response(self) -> Result<Block, Error> {
        Ok(Block(
            self.block.ok_or(Error::FailedToParseResponse)?.try_into()?,
        ))
    }
}

impl FromGrpcResponse<Block> for GetLatestBlockResponse {
    fn try_from_response(self) -> Result<Block, Error> {
        Ok(Block(
            self.block.ok_or(Error::FailedToParseResponse)?.try_into()?,
        ))
    }
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

impl FromGrpcResponse<BaseAccount> for QueryAccountResponse {
    fn try_from_response(self) -> Result<BaseAccount, Error> {
        let account = self.account.ok_or(Error::FailedToParseResponse)?;
        if account.type_url != RawBaseAccount::type_url() {
            return Err(Error::UnexpectedResponseType(account.type_url));
        }
        println!("ACTT: {:#?}", account.value);
        let base_account =
            RawBaseAccount::decode(&*account.value).map_err(|_| Error::FailedToParseResponse)?;

        let any_pub_key = base_account.pub_key.ok_or(Error::FailedToParseResponse)?;
        // cosmrs has different Any type than pbjson_types Any
        let pub_key = PublicKey::try_from(Any {
            type_url: any_pub_key.type_url,
            value: any_pub_key.value.to_vec(),
        })?;

        /*
        println!("key: {:#?}", raw_pub_key);
        let any_pub_key = Any::decode(raw_pub_key.value).unwrap();

        //if raw_pub_key.type_url != RawPublicKey::type_url() { return Err(Error::UnexpectedResponseType(raw_pub_key.type_url)); }
        panic!("xxx");
        //let a : () = raw_pub_key.value;
        let raw_pub_key = RawPublicKey::decode( &*raw_pub_key.value).unwrap();
        let pub_key = match raw_pub_key.sum.ok_or(Error::FailedToParseResponse)? {
            Sum::Ed25519(bytes) => {
                PublicKey::from_raw_ed25519(&bytes).ok_or(Error::FailedToParseResponse)?
            }
            #[cfg(feature = "secp256k1")]
            Sum::Secp256k1(bytes) => {
                PublicKey::from_raw_secp256k1(&bytes).ok_or(Error::FailedToParseResponse)?
            }
        };
        */
        Ok(BaseAccount {
            address: base_account.address,
            pub_key,
            account_number: base_account.account_number,
            sequence: base_account.sequence,
        })
    }
}

impl FromGrpcResponse<f64> for ConfigResponse {
    fn try_from_response(self) -> Result<f64, Error> {
        const UNITS_SUFFIX: &str = "utia";

        let min_gas_price_with_suffix = self.minimum_gas_price;
        let min_gas_price_str = min_gas_price_with_suffix
            .strip_suffix(UNITS_SUFFIX)
            .ok_or(Error::FailedToParseResponse)?;
        let min_gas_price = min_gas_price_str
            .parse::<f64>()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(min_gas_price)
    }
}

impl IntoGrpcParam<GetBlockByHeightRequest> for i64 {
    fn into_parameter(self) -> GetBlockByHeightRequest {
        GetBlockByHeightRequest { height: self }
    }
}

impl IntoGrpcParam<QueryAccountRequest> for String {
    fn into_parameter(self) -> QueryAccountRequest {
        QueryAccountRequest { address: self }
    }
}

/*
impl TryFrom<ConfigResponse> for GasPrice {
    type Error = Error;

    fn try_from(item: ConfigResponse) -> Result<Self, Self::Error> {
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
*/

/*
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
*/
