#![allow(clippy::all)]

pub mod defaults;
pub mod serializers;

include!(concat!(env!("OUT_DIR"), "/mod.rs"));
