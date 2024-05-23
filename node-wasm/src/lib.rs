#![doc = include_str!("../README.md")]
#![cfg(target_arch = "wasm32")]

pub mod error;
pub mod node;
pub mod utils;
mod wrapper;
