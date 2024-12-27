#![doc = include_str!("../README.md")]

mod error;
pub mod grpc;
#[cfg(target_arch = "wasm32")]
mod js_client;
mod tx;
mod utils;

pub use crate::error::{Error, Result};
pub use crate::grpc::GrpcClient;
pub use crate::tx::{TxClient, TxConfig};
