#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub mod blob;
mod block;
mod byzantine;
pub mod consts;
mod data_availability_header;
mod error;
mod extended_header;
pub mod fraud_proof;
pub mod hash;
pub mod namespaced_data;
pub mod nmt;
#[cfg(feature = "p2p")]
#[cfg_attr(docsrs, doc(cfg(feature = "p2p")))]
pub mod p2p;
pub mod row;
mod rsmt2d;
pub mod sample;
mod share;
pub mod state;
mod sync;
#[cfg(any(test, feature = "test-utils"))]
#[cfg_attr(docsrs, doc(cfg(feature = "test-utils")))]
pub mod test_utils;
pub mod trust_level;
pub mod tx_config;
mod validate;
mod validator_set;

pub use crate::blob::{Blob, Commitment};
pub use crate::block::*;
pub use crate::data_availability_header::*;
pub use crate::error::*;
pub use crate::extended_header::*;
pub use crate::fraud_proof::FraudProof;
pub use crate::rsmt2d::{AxisType, ExtendedDataSquare};
pub use crate::share::*;
pub use crate::sync::*;
pub use crate::validate::*;
