#![doc = include_str!("../README.md")]

mod abci_proofs;
mod builder;
mod client;
mod error;
pub mod grpc;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod js_client;
pub mod signer;
mod tx;
#[cfg(all(not(target_arch = "wasm32"), feature = "uniffi"))]
pub mod uniffi_client;
mod utils;

pub use crate::builder::GrpcClientBuilder;
pub use crate::client::GrpcClient;
pub use crate::error::GrpcClientBuilderError;
pub use crate::error::{Error, Result};
pub use crate::signer::DocSigner;
pub use crate::tx::{SignDoc, TxConfig, TxInfo};
pub use celestia_types::any::IntoProtobufAny;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
