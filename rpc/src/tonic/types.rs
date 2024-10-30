use prost::{Message, Name};

use celestia_types::auth::{AuthParams, BaseAccount};
use celestia_types::blob::BlobParams;

use celestia_proto::celestia::blob::v1::{
    QueryParamsRequest as QueryBlobParamsRequest, QueryParamsResponse as QueryBlobParamsResponse,
};
use celestia_proto::cosmos::auth::v1beta1::{
    BaseAccount as RawBaseAccount, QueryAccountRequest, QueryAccountResponse,
};
use celestia_proto::cosmos::auth::v1beta1::{
    QueryParamsRequest as QueryAuthParamsRequest, QueryParamsResponse as QueryAuthParamsResponse,
};
use celestia_proto::cosmos::base::node::v1beta1::{ConfigRequest, ConfigResponse};
use celestia_proto::cosmos::base::tendermint::v1beta1::{
    GetBlockByHeightRequest, GetBlockByHeightResponse, GetLatestBlockRequest,
    GetLatestBlockResponse,
};
use celestia_proto::cosmos::tx::v1beta1::{BroadcastMode, BroadcastTxRequest, GetTxRequest};

use celestia_tendermint::block::Block as TendermintBlock;

use celestia_tendermint_proto::v0_34::types::BlobTx;

use cosmrs::crypto::PublicKey;
use cosmrs::Any;

use crate::tonic::Error;

pub mod tx;

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

impl IntoGrpcParam<BroadcastTxRequest> for (BlobTx, BroadcastMode) {
    fn into_parameter(self) -> BroadcastTxRequest {
        let (blob_tx, mode) = self;
        BroadcastTxRequest {
            tx_bytes: blob_tx.encode_to_vec(),
            mode: mode.into(),
        }
    }
}

impl IntoGrpcParam<GetTxRequest> for String {
    fn into_parameter(self) -> GetTxRequest {
        GetTxRequest { hash: self }
    }
}

macro_rules! make_empty_params {
    ($request_type:ident) => {
        impl IntoGrpcParam<$request_type> for () {
            fn into_parameter(self) -> $request_type {
                $request_type {}
            }
        }
    };
}

make_empty_params!(GetLatestBlockRequest);
make_empty_params!(ConfigRequest);
make_empty_params!(QueryAuthParamsRequest);
make_empty_params!(QueryBlobParamsRequest);
