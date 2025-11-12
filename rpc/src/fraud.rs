//! celestia-node rpc types and methods related to fraud proofs

use std::future::Future;
use std::marker::{Send, Sync};
use std::pin::Pin;

use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use jsonrpsee::core::client::{ClientT, Error, SubscriptionClientT};
use jsonrpsee::proc_macros::rpc;

use crate::{HeaderClient, custom_client_error};

pub use celestia_types::fraud_proof::{Proof, ProofType};

mod rpc {
    use super::*;

    #[rpc(client, namespace = "fraud", namespace_separator = ".")]
    pub trait Fraud {
        #[method(name = "Get")]
        async fn fraud_get(&self, proof_type: ProofType) -> Result<Vec<Proof>, Error>;
    }

    #[rpc(client, namespace = "fraud", namespace_separator = ".")]
    pub trait FraudSubscription {
        #[subscription(name = "Subscribe", unsubscribe = "Unsubscribe", item = Proof)]
        async fn fraud_subscribe(&self, proof_type: ProofType) -> SubscriptionResult;
    }
}

/// Client implementation for the `Fraud` RPC API.
pub trait FraudClient: ClientT {
    /// Fetches fraud proofs by their type.
    fn fraud_get<'a, 'fut>(
        &'a self,
        proof_type: ProofType,
    ) -> impl Future<Output = Result<Vec<Proof>, Error>> + Send + 'fut
    where
        'a: 'fut,
        Self: Sized + Sync + 'fut,
    {
        rpc::FraudClient::fraud_get(self, proof_type)
    }

    /// Subscribe to fraud proof by its type.
    ///
    /// # Notes
    ///
    /// If client returns [`Error::HttpNotImplemented`], the subscription will fallback to
    /// using combination of [`HeaderClient::header_wait_for_height`] and
    /// [`FraudClient::fraud_get`] for streaming the proofs. The fallback stream will end
    /// after the first batch of proofs is returned.
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    fn fraud_subscribe<'a>(
        &'a self,
        proof_type: ProofType,
    ) -> Pin<Box<dyn Stream<Item = Result<Proof, Error>> + Send + 'a>>
    where
        Self: SubscriptionClientT + Sized + Sync,
    {
        try_stream! {
            let subscription_res = rpc::FraudSubscriptionClient::fraud_subscribe(self, proof_type).await;
            let has_real_sub = !matches!(&subscription_res, Err(Error::HttpNotImplemented));

            let (mut fraud_sub, mut header_sub) = if has_real_sub {
                (Some(subscription_res?), None)
            } else {
                (None, Some(HeaderClient::header_subscribe(self)))
            };

            loop {
                if has_real_sub {
                    yield fraud_sub
                        .as_mut()
                        .expect("must be some")
                        .next()
                        .await
                        .ok_or_else(|| custom_client_error("unexpected end of stream"))??;
                } else {
                    // tick; we don't care about the header
                    header_sub
                        .as_mut()
                        .expect("must be some")
                        .next()
                        .await
                        .ok_or_else(|| custom_client_error("unexpected end of stream"))??;

                    let proofs = rpc::FraudClient::fraud_get(self, proof_type).await?;
                    if !proofs.is_empty() {
                        for proof in proofs {
                            yield proof;
                        }

                        // after we got some proofs from the node, it would
                        // keep giving us the same proofs again and again,
                        // so we just end the stream here
                        break;
                    }
                };
            }
        }
        .boxed()
    }
}

impl<T> FraudClient for T where T: ClientT {}
