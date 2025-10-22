//! celestia-node rpc types and methods related to fraud proofs

use std::future::Future;
use std::marker::{Send, Sync};
use std::pin::Pin;

use async_stream::try_stream;
use futures::Stream;
use futures::StreamExt;
#[cfg(not(target_arch = "wasm32"))]
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::client::{ClientT, Error};
use jsonrpsee::proc_macros::rpc;

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
    /// On native this method forwards all the proofs that were submitted to the corresponding
    /// libp2p pubsub topic, regardless if the proof would pass the verification by
    /// node or not.
    ///
    /// On wasm this method will wait for any proofs to be successfully verified and stored by
    /// the node. Then it will return all the proofs node stored and the stream will be closed.
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[cfg(not(target_arch = "wasm32"))]
    fn fraud_subscribe<'a>(
        &'a self,
        proof_type: ProofType,
    ) -> Pin<Box<dyn Stream<Item = Result<Proof, Error>> + Send + 'a>>
    where
        Self: SubscriptionClientT + Sized + Sync,
    {
        try_stream! {
            let mut subscription = rpc::FraudSubscriptionClient::fraud_subscribe(self, proof_type).await?;

            while let Some(proof) = subscription.next().await {
                yield proof?;
            }
        }
        .boxed()
    }

    /// Subscribe to fraud proof by its type.
    ///
    /// # Notes
    ///
    /// On native this method forwards all the proofs that were submitted to the corresponding
    /// libp2p pubsub topic, regardless if the proof would pass the verification by
    /// node or not.
    ///
    /// On wasm this method will wait for any proofs to be successfully verified and stored by
    /// the node. Then it will return all the proofs node stored and the stream will be closed.
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[cfg(target_arch = "wasm32")]
    fn fraud_subscribe<'a>(
        &'a self,
        proof_type: ProofType,
    ) -> Pin<Box<dyn Stream<Item = Result<Proof, Error>> + Send + 'a>>
    where
        Self: Sized + Sync,
    {
        try_stream! {
            let mut subscription = super::HeaderClient::header_subscribe(self);

            loop {
                subscription.next().await.expect("headers never end")?;
                let proofs = rpc::FraudClient::fraud_get(self, proof_type).await?;

                if !proofs.is_empty() {
                    for proof in proofs {
                        yield proof;
                    }
                    break;
                }
            }
        }
        .boxed()
    }
}

impl<T> FraudClient for T where T: ClientT {}
