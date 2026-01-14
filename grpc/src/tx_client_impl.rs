use std::sync::Arc;

use async_trait::async_trait;
use k256::ecdsa::VerifyingKey;
use prost::Message;
use tendermint::chain::Id;
use tokio::sync::OnceCell;

use celestia_types::any::IntoProtobufAny;
use celestia_types::blob::{MsgPayForBlobs, RawBlobTx, RawMsgPayForBlobs};
use celestia_types::state::auth::BaseAccount;
use celestia_types::state::RawTxBody;

use crate::grpc::{GasEstimate, TxPriority};
use crate::signer::{BoxedDocSigner, sign_tx};
use crate::tx_client_v2::{SignFn, Transaction, TxCallbacks, TxRequest};
use crate::{Error, Result, TxConfig};

const BLOB_TX_TYPE_ID: &str = "BLOB";

#[async_trait]
pub(crate) trait SignContext: Send + Sync {
    async fn get_account(&self) -> Result<BaseAccount>;
    async fn chain_id(&self) -> Result<Id>;
    async fn estimate_gas_price(&self, priority: TxPriority) -> Result<f64>;
    async fn estimate_gas_price_and_usage(
        &self,
        priority: TxPriority,
        tx_bytes: Vec<u8>,
    ) -> Result<GasEstimate>;
}

pub(crate) struct SignFnBuilder {
    context: Arc<dyn SignContext>,
    pubkey: VerifyingKey,
    signer: Arc<BoxedDocSigner>,
}

impl SignFnBuilder {
    pub(crate) fn new(
        context: Arc<dyn SignContext>,
        pubkey: VerifyingKey,
        signer: Arc<BoxedDocSigner>,
    ) -> Self {
        Self {
            context,
            pubkey,
            signer,
        }
    }

    pub(crate) fn build(self) -> Arc<dyn SignFn> {
        Arc::new(BuiltSignFn {
            context: self.context,
            pubkey: self.pubkey,
            signer: self.signer,
            cached_account: OnceCell::new(),
            cached_chain_id: OnceCell::new(),
        })
    }
}

struct BuiltSignFn {
    context: Arc<dyn SignContext>,
    pubkey: VerifyingKey,
    signer: Arc<BoxedDocSigner>,
    cached_account: OnceCell<BaseAccount>,
    cached_chain_id: OnceCell<Id>,
}

#[async_trait]
impl SignFn for BuiltSignFn {
    async fn sign(&self, sequence: u64, request: &TxRequest, cfg: &TxConfig) -> Result<Transaction> {
        let chain_id = self
            .cached_chain_id
            .get_or_try_init(|| async { self.context.chain_id().await })
            .await?
            .clone();
        let base = self
            .cached_account
            .get_or_try_init(|| async { self.context.get_account().await })
            .await?
            .clone();

        let mut account = base.clone();
        account.sequence = sequence;
        let (tx_body, blobs) = match request {
            TxRequest::Blobs(blobs) => {
                let pfb = MsgPayForBlobs::new(blobs, account.address)
                    .map_err(Error::CelestiaTypesError)?;
                let tx_body = RawTxBody {
                    messages: vec![RawMsgPayForBlobs::from(pfb).into_any()],
                    memo: cfg.memo.clone().unwrap_or_default(),
                    ..RawTxBody::default()
                };
                (tx_body, Some(blobs))
            }
            TxRequest::Message(msg) => {
                let tx_body = RawTxBody {
                    messages: vec![msg.clone()],
                    memo: cfg.memo.clone().unwrap_or_default(),
                    ..RawTxBody::default()
                };
                (tx_body, None)
            }
            TxRequest::RawPayload(_) => {
                return Err(Error::UnexpectedResponseType(
                    "raw payload not supported".into(),
                ));
            }
        };

        let (gas_limit, gas_price) = match cfg.gas_limit {
            Some(gas_limit) => {
                let gas_price = match cfg.gas_price {
                    Some(price) => price,
                    None => self.context.estimate_gas_price(cfg.priority).await?,
                };
                (gas_limit, gas_price)
            }
            None => {
                let probe_tx = sign_tx(
                    tx_body.clone(),
                    chain_id.clone(),
                    &account,
                    &self.pubkey,
                    &*self.signer,
                    0,
                    1,
                )
                .await?;
                let GasEstimate { price, usage } = self
                    .context
                    .estimate_gas_price_and_usage(cfg.priority, probe_tx.encode_to_vec())
                    .await?;
                let gas_price = cfg.gas_price.unwrap_or(price);
                (usage, gas_price)
            }
        };
        let fee = (gas_limit as f64 * gas_price).ceil() as u64;

        let tx = sign_tx(
            tx_body,
            chain_id,
            &account,
            &self.pubkey,
            &*self.signer,
            gas_limit,
            fee,
        )
        .await?;
        let bytes = match blobs {
            Some(blobs) => {
                let blob_tx = RawBlobTx {
                    tx: tx.encode_to_vec(),
                    blobs: blobs.iter().cloned().map(Into::into).collect(),
                    type_id: BLOB_TX_TYPE_ID.to_string(),
                };
                blob_tx.encode_to_vec()
            }
            None => tx.encode_to_vec(),
        };
        Ok(Transaction {
            sequence,
            bytes,
            callbacks: TxCallbacks::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use k256::ecdsa::SigningKey;
    use rand::rngs::OsRng;
    use tendermint::chain::Id;
    use tendermint_proto::google::protobuf::Any;

    use celestia_types::state::{AccAddress, RawTxBody};

    struct MockContext {
        account: BaseAccount,
        chain_id: Id,
        gas_price: f64,
    }

    #[async_trait]
    impl SignContext for MockContext {
        async fn get_account(&self) -> Result<BaseAccount> {
            Ok(self.account.clone())
        }

        async fn chain_id(&self) -> Result<Id> {
            Ok(self.chain_id.clone())
        }

        async fn estimate_gas_price(&self, _priority: TxPriority) -> Result<f64> {
            Ok(self.gas_price)
        }

        async fn estimate_gas_price_and_usage(
            &self,
            _priority: TxPriority,
            _tx_bytes: Vec<u8>,
        ) -> Result<GasEstimate> {
            Ok(GasEstimate {
                price: self.gas_price,
                usage: 123,
            })
        }
    }

    #[tokio::test]
    async fn signfn_builder_signs_message() {
        let signing_key = SigningKey::random(&mut OsRng);
        let pubkey = signing_key.verifying_key().clone();
        let signer = Arc::new(BoxedDocSigner::new(signing_key));
        let address = AccAddress::from(pubkey);
        let account = BaseAccount {
            address,
            pub_key: None,
            account_number: 1,
            sequence: 0,
        };
        let context = Arc::new(MockContext {
            account,
            chain_id: Id::try_from("test-chain").expect("chain id"),
            gas_price: 0.1,
        });

        let sign_fn = SignFnBuilder::new(context, pubkey, signer).build();
        let tx = sign_fn
            .sign(
                1,
                &TxRequest::Message(Any {
                    type_url: "/test.Msg".into(),
                    value: RawTxBody::default().encode_to_vec(),
                }),
                &TxConfig::default().with_gas_limit(100),
            )
            .await
            .expect("sign tx");

        assert_eq!(tx.sequence, 1);
        assert!(!tx.bytes.is_empty());
    }

    #[tokio::test]
    async fn signfn_builder_rejects_raw_payload() {
        let signing_key = SigningKey::random(&mut OsRng);
        let pubkey = signing_key.verifying_key().clone();
        let signer = Arc::new(BoxedDocSigner::new(signing_key));
        let address = AccAddress::from(pubkey);
        let account = BaseAccount {
            address,
            pub_key: None,
            account_number: 1,
            sequence: 0,
        };
        let context = Arc::new(MockContext {
            account,
            chain_id: Id::try_from("test-chain").expect("chain id"),
            gas_price: 0.1,
        });

        let sign_fn = SignFnBuilder::new(context, pubkey, signer).build();
        let err = sign_fn
            .sign(
                1,
                &TxRequest::RawPayload(vec![1, 2, 3]),
                &TxConfig::default().with_gas_limit(100),
            )
            .await
            .expect_err("raw payload");

        assert!(matches!(err, Error::UnexpectedResponseType(_)));
    }
}
