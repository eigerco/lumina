#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub mod blob;
pub mod blobstream;
pub mod client;
mod error;
mod header;
#[cfg(feature = "p2p")]
mod p2p;
pub mod share;
mod state;
mod tx_config;

pub use crate::blob::BlobClient;
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
pub use crate::error::{Error, Result};
pub use crate::header::HeaderClient;
#[cfg(feature = "p2p")]
#[cfg_attr(docsrs, doc(cfg(feature = "p2p")))]
pub use crate::p2p::P2PClient;
pub use crate::share::ShareClient;
pub use crate::state::StateClient;
pub use crate::tx_config::TxConfig;

/// Re-exports of all the RPC traits.
pub mod prelude {
    pub use crate::BlobClient;
    pub use crate::HeaderClient;
    #[cfg(feature = "p2p")]
    pub use crate::P2PClient;
    pub use crate::ShareClient;
    pub use crate::StateClient;
}
