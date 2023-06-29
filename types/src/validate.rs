use crate::ValidationError;

pub trait ValidateBasic {
    fn validate_basic(&self) -> Result<(), ValidationError>;
}
