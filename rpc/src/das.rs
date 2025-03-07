use celestia_types::SamplingStats;
use jsonrpsee::proc_macros::rpc;

/// The `Das` RPC trait provides methods for interacting with Data Availability Sampling.
#[rpc(client)]
pub trait Das {
    /// Retrieves the current statistics over the DA sampling process.
    #[method(name = "das.SamplingStats")]
    async fn das_sampling_stats(&self) -> Result<SamplingStats, Error>;

    /// Blocks until DASer finishes catching up to the network head.
    #[method(name = "das.WaitCatchUp")]
    async fn das_wait_catch_up(&self) -> Result<(), Error>;
}
