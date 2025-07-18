use std::sync::Arc;

use async_stream::try_stream;
use celestia_rpc::fraud::{Proof, ProofType};
use celestia_rpc::FraudClient;
use futures_util::Stream;

use crate::client::Context;
use crate::Result;

/// Fraud API for quering bridge nodes.
pub struct FraudApi {
    ctx: Arc<Context>,
}

impl FraudApi {
    pub(crate) fn new(ctx: Arc<Context>) -> FraudApi {
        FraudApi { ctx }
    }

    /// Fetches fraud proofs from node by their type.
    pub async fn get(&self, proof_type: ProofType) -> Result<Vec<Proof>> {
        Ok(self.ctx.rpc.fraud_get(proof_type).await?)
    }

    /// Subscribe to fraud proof by its type.
    pub async fn subscribe(&self, proof_type: ProofType) -> impl Stream<Item = Result<Proof>> {
        let ctx = self.ctx.clone();

        try_stream! {
            let mut subscription = ctx.rpc.fraud_subscribe(proof_type).await?;

            while let Some(item) = subscription.next().await {
                let proof = item?;
                // TODO: Should we validate proof?
                yield proof;
            }
        }
    }
}
