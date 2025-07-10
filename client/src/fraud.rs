use std::sync::Arc;

use celestia_rpc::FraudClient;
use futures_util::{Stream, TryStreamExt};

pub use celestia_rpc::fraud::{Proof, ProofType};

use crate::client::Context;
use crate::Result;

pub struct FraudApi {
    ctx: Arc<Context>,
}

impl FraudApi {
    pub(crate) fn new(ctx: Arc<Context>) -> FraudApi {
        FraudApi { ctx }
    }

    /// Fetches fraud proofs from node by their type.
    async fn get(&self, proof_type: ProofType) -> Result<Vec<Proof>> {
        Ok(self.ctx.rpc.fraud_get(proof_type).await?)
    }

    /// Subscribe to fraud proof by its type.
    async fn subscribe(&self, proof_type: ProofType) -> Result<impl Stream<Item = Result<Proof>>> {
        Ok(self
            .ctx
            .rpc
            .fraud_subscribe(proof_type)
            .await?
            .map_err(Into::into))
    }
}
