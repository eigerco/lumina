use celestia_proto::celestia::core::v1::gas_estimation::{
    EstimateGasPriceAndUsageRequest, EstimateGasPriceAndUsageResponse, EstimateGasPriceRequest,
    EstimateGasPriceResponse,
};
use serde::{Deserialize, Serialize};

use crate::grpc::{FromGrpcResponse, IntoGrpcParam};
use crate::Result;

/// TxPriority is the priority level of the requested gas price.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[cfg_attr(
    all(feature = "wasm-bindgen", target_arch = "wasm32"),
    wasm_bindgen::prelude::wasm_bindgen
)]
#[repr(i32)]
pub enum TxPriority {
    /// Estimated gas price is the value at the end of the lowest 10% of gas prices from the last 5 blocks.
    Low = 1,
    /// Estimated gas price is the mean of all gas prices from the last 5 blocks.
    #[default]
    Medium = 2,
    /// Estimated gas price is the price at the start of the top 10% of transactions’ gas prices from the last 5 blocks.
    High = 3,
}

/// Result of gas price and usage estimation
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(
    all(feature = "wasm-bindgen", target_arch = "wasm32"),
    wasm_bindgen::prelude::wasm_bindgen
)]
pub struct GasEstimate {
    /// Gas price estimated based on last 5 blocks
    pub price: f64,
    /// Simulated transaction gas usage
    pub usage: u64,
}

impl IntoGrpcParam<EstimateGasPriceRequest> for TxPriority {
    fn into_parameter(self) -> EstimateGasPriceRequest {
        EstimateGasPriceRequest {
            tx_priority: self as i32,
        }
    }
}

impl FromGrpcResponse<f64> for EstimateGasPriceResponse {
    fn try_from_response(self) -> Result<f64> {
        Ok(self.estimated_gas_price)
    }
}

impl IntoGrpcParam<EstimateGasPriceAndUsageRequest> for (TxPriority, Vec<u8>) {
    fn into_parameter(self) -> EstimateGasPriceAndUsageRequest {
        EstimateGasPriceAndUsageRequest {
            tx_priority: self.0 as i32,
            tx_bytes: self.1,
        }
    }
}

impl FromGrpcResponse<GasEstimate> for EstimateGasPriceAndUsageResponse {
    fn try_from_response(self) -> Result<GasEstimate> {
        Ok(GasEstimate {
            price: self.estimated_gas_price,
            usage: self.estimated_gas_used,
        })
    }
}
