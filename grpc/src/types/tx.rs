use std::convert::Infallible;

use k256::ecdsa::{signature::Signer, Signature};
use pbjson_types::Any;
use prost::{Message, Name};

use celestia_proto::cosmos::crypto::secp256k1;
use celestia_proto::cosmos::tx::v1beta1::{
    BroadcastTxRequest, BroadcastTxResponse, GetTxRequest as RawGetTxRequest,
    GetTxResponse as RawGetTxResponse, SignDoc,
};
use celestia_tendermint::public_key::Secp256k1 as VerifyingKey;
use celestia_tendermint_proto::Protobuf;
use celestia_types::auth::BaseAccount;
use celestia_types::blob::{Blob, RawBlob, RawBlobTx};
use celestia_types::state::{
    AuthInfo, Fee, ModeInfo, RawTx, RawTxBody, SignerInfo, Sum, Tx, TxResponse,
};

use crate::types::{FromGrpcResponse, IntoGrpcParam};
use crate::Error;

pub use celestia_proto::cosmos::tx::v1beta1::BroadcastMode;

/// Response to GetTx
#[derive(Debug)]
pub struct GetTxResponse {
    /// Response Transaction
    pub tx: Tx,

    /// TxResponse to a Query
    pub tx_response: TxResponse,
}

impl FromGrpcResponse<TxResponse> for BroadcastTxResponse {
    fn try_from_response(self) -> Result<TxResponse, Error> {
        Ok(self
            .tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?)
    }
}

impl FromGrpcResponse<GetTxResponse> for RawGetTxResponse {
    fn try_from_response(self) -> Result<GetTxResponse, Error> {
        let tx_response = self
            .tx_response
            .ok_or(Error::FailedToParseResponse)?
            .try_into()?;

        let tx = self.tx.ok_or(Error::FailedToParseResponse)?;

        let cosmos_tx = Tx {
            body: tx.body.ok_or(Error::FailedToParseResponse)?.try_into()?,
            auth_info: tx
                .auth_info
                .ok_or(Error::FailedToParseResponse)?
                .try_into()?,
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

        // From https://github.com/celestiaorg/celestia-core/blob/v1.43.0-tm-v0.34.35/pkg/consts/consts.go#L19
        const BLOB_TX_TYPE_ID: &str = "BLOB";

        // empty blob list causes error response, but this is already checked when creating MsgPayForBlobs
        debug_assert!(!blobs.is_empty());

        let blobs = blobs.into_iter().map(RawBlob::from).collect();
        let blob_tx = RawBlobTx {
            tx: tx.encode_to_vec(),
            blobs,
            type_id: BLOB_TX_TYPE_ID.to_string(),
        };
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

/// Sign `tx_body` and the transaction metadata as the `base_account` using `signer`
pub fn sign_tx(
    tx_body: RawTxBody,
    chain_id: String,
    base_account: &BaseAccount,
    verifying_key: VerifyingKey,
    signer: impl Signer<Signature>,
    gas_limit: u64,
    fee: u64,
) -> RawTx {
    // From https://github.com/celestiaorg/cosmos-sdk/blob/v1.25.0-sdk-v0.46.16/proto/cosmos/tx/signing/v1beta1/signing.proto#L24
    const SIGNING_MODE_INFO: ModeInfo = ModeInfo {
        sum: Sum::Single { mode: 1 },
    };

    let public_key = secp256k1::PubKey {
        key: verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    };
    let public_key_as_any = Any {
        type_url: secp256k1::PubKey::type_url(),
        value: public_key.encode_to_vec().into(),
    };

    let auth_info = AuthInfo {
        signer_infos: vec![SignerInfo {
            public_key: Some(public_key_as_any),
            mode_info: SIGNING_MODE_INFO,
            sequence: base_account.sequence,
        }],
        fee: Fee::new(fee, gas_limit),
    };
    let auth_info_bytes: Result<_, Infallible> = auth_info.encode_vec();

    let bytes_to_sign = SignDoc {
        body_bytes: tx_body.encode_to_vec(),
        auth_info_bytes: auth_info_bytes.expect("Result to be Infallible"),
        chain_id,
        account_number: base_account.account_number,
    }
    .encode_to_vec();

    let signature: Signature = signer.sign(&bytes_to_sign);

    RawTx {
        auth_info: Some(auth_info.into()),
        body: Some(tx_body),
        signatures: vec![signature.to_bytes().to_vec()],
    }
}
