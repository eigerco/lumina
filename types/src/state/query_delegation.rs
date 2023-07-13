use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDelegationResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryUnbondingDelegationResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRedelegationsResponse;
