#![doc = include_str!("../README.md")]
#![cfg(target_arch = "wasm32")]

pub mod client;
mod commands;
pub mod error;
mod ports;
pub(crate) mod subscriptions;
pub mod utils;
mod worker;
mod worker_client;
mod worker_server;
mod wrapper;

// include celestia-grpc when building lumina-node npm package
pub use celestia_grpc as grpc;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
