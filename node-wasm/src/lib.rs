#![doc = include_str!("../README.md")]
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsError;

pub mod node;
pub mod utils;
mod wrapper;

/// Alias for a `Result` with the error type [`JsError`].
pub type Result<T, E = JsError> = std::result::Result<T, E>;
