#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

use std::fmt;

use jsonrpsee::core::client::Error as JsonrpseeError;

pub mod blob;
pub mod blobstream;
pub mod client;
pub mod das;
mod error;
pub mod fraud;
mod header;
#[cfg(feature = "p2p")]
/// Types and client for the p2p JSON-RPC API.
pub mod p2p;
pub mod share;
mod state;
mod tx_config;

pub use crate::blob::BlobClient;
pub use crate::blobstream::BlobstreamClient;
#[cfg(any(
    not(target_arch = "wasm32"),
    all(target_arch = "wasm32", feature = "wasm-bindgen")
))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(
        not(target_arch = "wasm32"),
        all(target_arch = "wasm32", feature = "wasm-bindgen")
    )))
)]
pub use crate::client::Client;
pub use crate::das::DasClient;
pub use crate::error::{Error, Result};
pub use crate::fraud::FraudClient;
pub use crate::header::HeaderClient;
#[cfg(feature = "p2p")]
#[cfg_attr(docsrs, doc(cfg(feature = "p2p")))]
pub use crate::p2p::P2PClient;
pub use crate::share::ShareClient;
pub use crate::state::StateClient;
pub use crate::tx_config::{TxConfig, TxPriority};

/// Re-exports of all the RPC traits.
pub mod prelude {
    pub use crate::BlobClient;
    pub use crate::BlobstreamClient;
    pub use crate::DasClient;
    pub use crate::FraudClient;
    pub use crate::HeaderClient;
    #[cfg(feature = "p2p")]
    pub use crate::P2PClient;
    pub use crate::ShareClient;
    pub use crate::StateClient;
}

// helper to map errors to jsonrpsee using Display
fn custom_client_error<E: fmt::Display>(error: E) -> JsonrpseeError {
    JsonrpseeError::Custom(error.to_string())
}
