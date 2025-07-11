use jsonrpsee::proc_macros::rpc;

pub use celestia_types::fraud_proof::{Proof, ProofType};

#[rpc(client, namespace = "fraud", namespace_separator = ".")]
pub trait Fraud {
    /// Fetches fraud proofs from by their type.
    #[method(name = "Get")]
    async fn fraud_get(&self, proof_type: ProofType) -> Result<Vec<Proof>, Error>;

    /// Subscribe to fraud proof by its type.
    ///
    /// # Notes
    ///
    /// Unsubscribe is not implemented by Celestia nodes.
    #[subscription(name = "Subscribe", unsubscribe = "Unsubscribe", item = Proof)]
    async fn fraud_subscribe(&self, proof_type: ProofType) -> SubscriptionResult;
}
