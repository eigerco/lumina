#![doc = include_str!("../README.md")]

#[cfg(feature = "transport")]
mod client;
mod error;
/// Custom types and wrappers needed by gRPC
pub mod types;

#[cfg(feature = "transport")]
pub use crate::client::GrpcClient;
pub use crate::error::{Error, Result};
