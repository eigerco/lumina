//! Custom serializers to be used with [`serde`].

pub mod null_default;
#[cfg(not(feature = "tonic"))]
pub mod option_any;
pub mod option_timestamp;
