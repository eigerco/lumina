use std::sync::Arc;

use crate::Context;

pub struct FraudApi {
    ctx: Arc<Context>,
}

impl FraudApi {
    pub(crate) fn new(ctx: Arc<Context>) -> FraudApi {
        FraudApi { ctx }
    }

    /*
    // Subscribe allows to subscribe on a Proof pub sub topic by its type.
    Subscribe(context.Context, fraud.ProofType) (<-chan *Proof, error)
    // Get fetches fraud proofs from the disk by its type.
    Get(context.Context, fraud.ProofType) ([]Proof, error)
     */
}
