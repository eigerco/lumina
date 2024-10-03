use crate::ValidationError;

/// A trait to perform basic validation of the data consistency.
pub trait ValidateBasic {
    /// Perform a basic validation of the data consistency.
    fn validate_basic(&self) -> Result<(), ValidationError>;
}

pub trait ValidateBasicWithAppVersion {
    fn validate_basic(&self, app_version: u64) -> Result<(), ValidationError>;
}
