use std::sync::Arc;

use celestia_grpc::DocSigner;

use crate::Context;

pub struct FraudApi<S> {
    ctx: Arc<Context<S>>,
}

impl<S> FraudApi<S>
where
    S: DocSigner,
{
    pub(crate) fn new(ctx: Arc<Context<S>>) -> FraudApi<S> {
        FraudApi { ctx }
    }

    /*
    // Subscribe allows to subscribe on a Proof pub sub topic by its type.
    Subscribe(context.Context, fraud.ProofType) (<-chan *Proof, error)
    // Get fetches fraud proofs from the disk by its type.
    Get(context.Context, fraud.ProofType) ([]Proof, error)
     */
}
