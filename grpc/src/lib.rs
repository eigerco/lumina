#![doc = include_str!("../README.md")]

mod error;
pub mod grpc;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod js_client;
mod tx;
#[cfg(feature = "uniffi")]
pub mod uniffi_client;
mod utils;

pub use crate::error::{Error, Result};
pub use crate::grpc::GrpcClient;
pub use crate::tx::{DocSigner, IntoAny, SignDoc, TxClient, TxConfig};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
#[cfg(feature = "uniffi")]
celestia_types::uniffi_reexport_scaffolding!();
