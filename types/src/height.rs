use tendermint::block::Height;

pub trait IntoHeight {
    // Convert self into a Height, returning an error if conversion fails
    fn into_height(self) -> Result<Height, HeightConversionError>;
}

// Error type for conversion failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeightConversionError {
    NegativeValue,
    Overflow,
}

impl std::fmt::Display for HeightConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeightConversionError::NegativeValue => write!(f, "Height cannot be negative"),
            HeightConversionError::Overflow => write!(f, "Value too large for Height"),
        }
    }
}

impl std::error::Error for HeightConversionError {}

// Implementations for different types
impl IntoHeight for Height {
    fn into_height(self) -> Result<Height, HeightConversionError> {
        Ok(self)
    }
}

impl IntoHeight for u64 {
    fn into_height(self) -> Result<Height, HeightConversionError> {
        Height::try_from(self).map_err(|_| HeightConversionError::Overflow)
    }
}

impl IntoHeight for i64 {
    fn into_height(self) -> Result<Height, HeightConversionError> {
        if self < 0 {
            Err(HeightConversionError::NegativeValue)
        } else {
            Ok(Height::try_from(self as u64).map_err(|_| HeightConversionError::Overflow)?)
        }
    }
}
