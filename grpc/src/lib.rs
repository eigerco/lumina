#![doc = include_str!("../README.md")]
#![cfg(not(target_arch = "wasm32"))]

mod client;
mod error;
/// Custom types and wrappers needed by gRPC
pub mod types;

pub use crate::client::GrpcClient;
pub use crate::error::{Error, Result};
