use std::convert::Infallible;

use celestia_tendermint_proto::google::protobuf::Any;
use cosmrs::Tx;
use k256::ecdsa::{signature::Signer, Signature};
use prost::{Message, Name};
use serde::{Deserialize, Serialize};

use celestia_proto::celestia::blob::v1::MsgPayForBlobs as RawMsgPayForBlobs;

use celestia_proto::cosmos::base::abci::v1beta1::AbciMessageLog;
use celestia_proto::cosmos::base::abci::v1beta1::TxResponse as RawTxResponse;
use celestia_proto::cosmos::base::v1beta1::Coin;
use celestia_proto::cosmos::crypto::secp256k1;
use celestia_proto::cosmos::tx::v1beta1::mode_info::{Single, Sum};
use celestia_proto::cosmos::tx::v1beta1::SignDoc;
use celestia_proto::cosmos::tx::v1beta1::{
    AuthInfo, BroadcastTxResponse, Fee, GetTxRequest as RawGetTxRequest,
    GetTxResponse as RawGetTxResponse, ModeInfo, SignerInfo, Tx as RawTx, TxBody,
};
use celestia_proto::cosmos::tx::v1beta1::{BroadcastMode, BroadcastTxRequest, GetTxRequest};
use celestia_tendermint_proto::v0_34::abci::Event;
use celestia_tendermint_proto::v0_34::types::{Blob as RawBlob, BlobTx as RawBlobTx};
use celestia_tendermint_proto::Protobuf;
use celestia_types::auth::{AccountKeypair, BaseAccount};
use celestia_types::blob::{Blob, MsgPayForBlobs};

use crate::types::{FromGrpcResponse, IntoGrpcParam};
use crate::Error;

/// Response to a tx query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse {
    /// The block height
    pub height: i64,

    /// The transaction hash.
    pub txhash: String,

    /// Namespace for the Code
    pub codespace: String,

    /// Response code.
    pub code: u32,

    /// Result bytes, if any.
    pub data: String,

    /// The output of the application's logger (raw string). May be
    /// non-deterministic.
    pub raw_log: String,

    /// The output of the application's logger (typed). May be non-deterministic.
    pub logs: Vec<AbciMessageLog>,

    /// Additional information. May be non-deterministic.
    pub info: String,

    /// Amount of gas requested for transaction.
    pub gas_wanted: i64,

    /// Amount of gas consumed by transaction.
    pub gas_used: i64,

    /// The request transaction bytes.
    pub tx: Option<Any>,

    /// Time of the previous block. For heights > 1, it's the weighted median of
    /// the timestamps of the valid votes in the block.LastCommit. For height == 1,
    /// it's genesis time.
    pub timestamp: String,

    /// Events defines all the events emitted by processing a transaction. Note,
    /// these events include those emitted by processing all the messages and those
    /// emitted from the ante. Whereas Logs contains the events, with
    /// additional metadata, emitted only by processing the messages.
    pub events: Vec<Event>,
}

/// Response to GetTx
#[derive(Debug)]
pub struct GetTxResponse {
    /// Response Transaction
    pub tx: Tx,

    /// TxResponse to a Query
    pub tx_response: TxResponse,
}

impl TryFrom<RawTxResponse> for TxResponse {
    type Error = Error;

    fn try_from(response: RawTxResponse) -> Result<TxResponse, Self::Error> {
        Ok(TxResponse {
            height: response.height,
            txhash: response.txhash,
            codespace: response.codespace,
            code: response.code,
            data: response.data,
            raw_log: response.raw_log,
            logs: response.logs,
            info: response.info,
            gas_wanted: response.gas_wanted,
            gas_used: response.gas_used,
            tx: response.tx,
            timestamp: response.timestamp,
            events: response.events,
        })
    }
}

impl FromGrpcResponse<TxResponse> for BroadcastTxResponse {
    fn try_from_response(self) -> Result<TxResponse, Error> {
        self.tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()
    }
}

impl FromGrpcResponse<GetTxResponse> for RawGetTxResponse {
    fn try_from_response(self) -> Result<GetTxResponse, Error> {
        let tx_response = self
            .tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?;

        let tx = self.tx.ok_or(Error::FailedToParseResponse)?;

        let cosmrs_tx_body: cosmos_sdk_proto::cosmos::tx::v1beta1::TxBody =
            tx.body.ok_or(Error::FailedToParseResponse)?.into();
        let cosmrs_auth_info: cosmos_sdk_proto::cosmos::tx::v1beta1::AuthInfo =
            tx.auth_info.ok_or(Error::FailedToParseResponse)?.into();

        let cosmos_tx = Tx {
            body: cosmrs_tx_body.try_into()?,
            auth_info: cosmrs_auth_info.try_into()?,
            signatures: tx.signatures,
        };

        Ok(GetTxResponse {
            tx: cosmos_tx,
            tx_response,
        })
    }
}

impl IntoGrpcParam<BroadcastTxRequest> for (RawTx, Vec<Blob>, BroadcastMode) {
    fn into_parameter(self) -> BroadcastTxRequest {
        let (tx, blobs, mode) = self;
        assert!(blobs.len() > 0);
        let blob_tx = new_blob_tx(&tx, blobs);
        BroadcastTxRequest {
            tx_bytes: blob_tx.encode_to_vec(),
            mode: mode.into(),
        }
    }
}

impl IntoGrpcParam<RawGetTxRequest> for String {
    fn into_parameter(self) -> RawGetTxRequest {
        RawGetTxRequest { hash: self }
    }
}

pub fn prep_signed_tx(
    msg_pay_for_blobs: &MsgPayForBlobs,
    base_account: &BaseAccount,
    gas_limit: u64,
    fee: u64,
    chain_id: String,
    account_keys: AccountKeypair,
) -> RawTx {
    // From https://github.com/celestiaorg/celestia-app/blob/v2.3.1/pkg/appconsts/global_consts.go#L77
    const FEE_DENOM: &str = "utia";
    // From https://github.com/celestiaorg/cosmos-sdk/blob/v1.25.0-sdk-v0.46.16/proto/cosmos/tx/signing/v1beta1/signing.proto#L24
    const SIGNING_MODE_INFO: Option<ModeInfo> = Some(ModeInfo {
        sum: Some(Sum::Single(Single { mode: 1 })),
    });

    let fee = Fee {
        amount: vec![Coin {
            denom: FEE_DENOM.to_string(),
            amount: fee.to_string(),
        }],
        gas_limit,
        ..Fee::default()
    };

    let public_key = secp256k1::PubKey {
        key: account_keys
            .verifying_key
            .to_encoded_point(true)
            .as_bytes()
            .to_vec(),
    };

    let public_key_as_any = Any {
        type_url: secp256k1::PubKey::type_url(),
        value: public_key.encode_to_vec().into(),
    };

    let auth_info = AuthInfo {
        signer_infos: vec![SignerInfo {
            public_key: Some(public_key_as_any.into()),
            mode_info: SIGNING_MODE_INFO,
            sequence: base_account.sequence,
        }],
        fee: Some(fee),
        tip: None,
    };

    let msg_pay_for_blobs_value: Result<_, Infallible> = msg_pay_for_blobs.encode_vec();
    let msg_pay_for_blobs_as_any = Any {
        type_url: RawMsgPayForBlobs::type_url(),
        value: msg_pay_for_blobs_value.expect("Result to be Infallible"),
    };

    let tx_body = TxBody {
        messages: vec![msg_pay_for_blobs_as_any],
        ..TxBody::default()
    };

    let bytes_to_sign = SignDoc {
        body_bytes: tx_body.encode_to_vec(),
        auth_info_bytes: auth_info.encode_to_vec(),
        chain_id,
        account_number: base_account.account_number,
    }
    .encode_to_vec();

    let signature: Signature = account_keys.signing_key.sign(&bytes_to_sign);

    RawTx {
        auth_info: Some(auth_info),
        body: Some(tx_body),
        signatures: vec![signature.to_bytes().to_vec()],
    }
}

pub fn new_blob_tx(signed_tx: &RawTx, blobs: Vec<Blob>) -> RawBlobTx {
    // From https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
    const BLOB_TX_TYPE_ID: &str = "BLOB";

    let blobs = blobs.into_iter().map(|blob| RawBlob::from(blob)).collect();
    RawBlobTx {
        tx: signed_tx.encode_to_vec(),
        blobs,
        type_id: BLOB_TX_TYPE_ID.to_string(),
    }
}
