use serde::{Deserialize, Serialize};
use tendermint::time::Time;

use crate::hash::Hash;

/// A state of the blockchain synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    /// The ID of the current syncing process.
    pub id: u64,
    /// Currently synced height.
    pub height: u64,
    /// The first height to be synced.
    pub from_height: u64,
    /// The last height to be synced.
    pub to_height: u64,
    /// The first hash to be synced.
    #[serde(with = "crate::serializers::hash")]
    pub from_hash: Hash,
    /// The last hash to be synced.
    #[serde(with = "crate::serializers::hash")]
    pub to_hash: Hash,
    /// The time when syncing began.
    pub start: Time,
    /// The time when syncing ended.
    pub end: Time,
    /// Any error during synchronisation, if it occured
    pub error: Option<String>,
}
