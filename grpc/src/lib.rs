#![doc = include_str!("../README.md")]

mod abci_proofs;
mod error;
pub mod grpc;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod js_client;
mod tx;
#[cfg(all(not(target_arch = "wasm32"), feature = "uniffi"))]
pub mod uniffi_client;
mod utils;

pub use crate::error::{Error, Result};
pub use crate::grpc::GrpcClient;
pub use crate::tx::{DocSigner, SignDoc, TxClient, TxConfig, TxInfo};
pub use celestia_types::any::IntoProtobufAny;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
