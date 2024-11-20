#![allow(clippy::all)]
#![allow(missing_docs)]
#![allow(rustdoc::invalid_rust_codeblocks)]
#![cfg(not(doctest))]
#![doc = include_str!("../README.md")]

pub mod serializers;

#[cfg(not(feature = "tonic"))]
include!(concat!(env!("OUT_DIR"), "/mod.rs"));

#[cfg(feature = "tonic")]
::tonic::include_proto!("mod");
