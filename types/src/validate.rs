use crate::consts::appconsts::AppVersion;
use crate::ValidationError;

/// A trait to perform basic validation of the data consistency.
pub trait ValidateBasic {
    /// Perform a basic validation of the data consistency.
    fn validate_basic(&self) -> Result<(), ValidationError>;
}

/// A trait to perform basic validation of the data consistency.
pub trait ValidateBasicWithAppVersion {
    /// Perform a basic validation of the data consistency.
    fn validate_basic(&self, app_version: AppVersion) -> Result<(), ValidationError>;
}
