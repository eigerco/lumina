#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsError;

pub mod node;
pub mod utils;
mod wrapper;

pub type Result<T> = std::result::Result<T, JsError>;
