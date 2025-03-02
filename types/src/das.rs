use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DasWorker {
    pub job_type: String,
    pub current: u64,
    pub from: u64,
    pub to: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingStats {
    pub head_of_sampled_chain: u64,
    pub head_of_catchup: u64,
    pub network_head_height: u64,
    pub workers: Option<Vec<DasWorker>>,
    pub concurrency: u64,
    pub catch_up_done: bool,
    pub is_running: bool,
}
