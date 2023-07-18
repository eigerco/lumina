use serde::{Deserialize, Serialize};
use tendermint::hash::Hash;
use tendermint::time::Time;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    pub id: u64,
    pub height: u64,
    pub from_height: u64,
    pub to_height: u64,
    pub from_hash: Hash,
    pub to_hash: Hash,
    pub start: Time,
    pub end: Time,
    pub error: Option<String>,
}
