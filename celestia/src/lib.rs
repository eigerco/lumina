#![cfg(not(target_arch = "wasm32"))]

mod native;
pub use crate::native::*;
