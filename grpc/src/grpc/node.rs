use celestia_proto::cosmos::base::node::v1beta1::{ConfigRequest, ConfigResponse};

use crate::grpc::{make_empty_params, make_response_identity, FromGrpcResponse};
use crate::{Error, Result};

impl FromGrpcResponse<f64> for ConfigResponse {
    fn try_from_response(self) -> Result<f64> {
        const UNITS_SUFFIX: &str = "utia";

        let min_gas_price_with_suffix = self.minimum_gas_price;
        let min_gas_price_str = min_gas_price_with_suffix
            .strip_suffix(UNITS_SUFFIX)
            .ok_or(Error::FailedToParseResponse)?;
        let min_gas_price = min_gas_price_str
            .parse::<f64>()
            .map_err(|_| Error::FailedToParseResponse)?;

        Ok(min_gas_price)
    }
}

make_response_identity!(ConfigResponse);

make_empty_params!(ConfigRequest);
