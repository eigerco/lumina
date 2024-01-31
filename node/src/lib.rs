#![cfg_attr(docs_rs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

mod executor;
pub mod network;
pub mod node;
pub mod p2p;
pub mod peer_tracker;
pub mod store;
pub mod syncer;
#[cfg(any(test, feature = "test-utils"))]
#[cfg_attr(docs_rs, doc(cfg(feature = "test-utils")))]
pub mod test_utils;
mod utils;
