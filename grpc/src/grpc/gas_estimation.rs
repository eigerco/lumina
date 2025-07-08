use celestia_proto::celestia::core::v1::gas_estimation::{
    EstimateGasPriceAndUsageRequest, EstimateGasPriceAndUsageResponse, EstimateGasPriceRequest,
    EstimateGasPriceResponse,
};

use crate::grpc::{FromGrpcResponse, IntoGrpcParam};
use crate::Result;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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

impl FromGrpcResponse<(f64, u64)> for EstimateGasPriceAndUsageResponse {
    fn try_from_response(self) -> Result<(f64, u64)> {
        Ok((self.estimated_gas_price, self.estimated_gas_used))
    }
}
