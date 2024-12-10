#![doc = include_str!("../README.md")]

mod client;
mod error;
pub mod types;

pub use crate::client::GrpcClient;
pub use crate::error::{Error, Result};
