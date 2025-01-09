#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub mod blob;
pub mod block;
mod byzantine;
pub mod consts;
mod data_availability_header;
pub mod eds;
mod error;
mod extended_header;
pub mod fraud_proof;
pub mod hash;
mod merkle_proof;
pub mod nmt;
#[cfg(feature = "p2p")]
#[cfg_attr(docsrs, doc(cfg(feature = "p2p")))]
pub mod p2p;
pub mod row;
pub mod row_namespace_data;
pub mod sample;
pub mod serializers;
mod share;
pub mod state;
mod sync;
#[cfg(any(test, feature = "test-utils"))]
#[cfg_attr(docsrs, doc(cfg(feature = "test-utils")))]
pub mod test_utils;
pub mod trust_level;
mod validate;
mod validator_set;

pub use crate::blob::{Blob, Commitment};
pub use crate::block::Height;
pub use crate::consts::appconsts::AppVersion;
pub use crate::data_availability_header::*;
pub use crate::eds::{AxisType, ExtendedDataSquare};
pub use crate::error::*;
pub use crate::extended_header::*;
pub use crate::fraud_proof::FraudProof;
pub use crate::merkle_proof::MerkleProof;
pub use crate::share::*;
pub use crate::sync::*;
pub use crate::validate::*;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
