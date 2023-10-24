#[cfg(target_arch = "wasm32")]
pub mod node;
#[cfg(target_arch = "wasm32")]
pub mod utils;

#[cfg(not(target_arch = "wasm32"))]
pub mod axum_server;
