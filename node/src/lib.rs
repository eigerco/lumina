#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub mod blockstore;
pub mod daser;
mod executor;
pub mod network;
pub mod node;
pub mod p2p;
pub mod peer_tracker;
pub mod store;
pub mod syncer;
#[cfg(any(test, feature = "test-utils"))]
#[cfg_attr(docsrs, doc(cfg(feature = "test-utils")))]
pub mod test_utils;
mod utils;

#[cfg(all(target_arch = "wasm32", test))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
