use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a worker responsible for DAS (Data Availability Sampling) jobs.
pub struct DasWorker {
    /// Type of the job the worker is performing.
    pub job_type: String,
    /// Current progress of the job.
    pub current: u64,
    /// The starting height of the job.
    pub from: u64,
    /// The ending height of the job.
    pub to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Contains statistics related to the DAS process.
pub struct SamplingStats {
    /// The height of the most recently sampled block in the chain.
    pub head_of_sampled_chain: u64,
    /// The height up to which catch-up has been completed.
    pub head_of_catchup: u64,
    /// The current height of the network's head.
    pub network_head_height: u64,
    /// The workers assigned to the DAS process.
    pub workers: Option<Vec<DasWorker>>,
    /// The number of concurrent DAS jobs running.
    pub concurrency: u64,
    /// Whether the DAS process has completed catching up.
    pub catch_up_done: bool,
    /// Whether the DAS process is actively running.
    pub is_running: bool,
}
