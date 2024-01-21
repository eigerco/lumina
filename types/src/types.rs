#[cfg(not(feature = "std"))]
pub use alloc::collections::{
    btree_map, btree_set, BTreeMap, BTreeMap as HashMap, BTreeSet, BTreeSet as HashSet,
};

#[cfg(not(feature = "std"))]
pub use alloc::vec::{self as vec, Vec};

#[cfg(feature = "std")]
pub use std::collections::{
    btree_map, btree_set, BTreeMap, BTreeMap as HashMap, BTreeSet as HashSet,
};

#[cfg(feature = "std")]
pub use std::vec::{self as vec, Vec};

#[cfg(not(feature = "std"))]
pub use alloc::string::{String, ToString};

#[cfg(feature = "std")]
pub use std::string::{String, ToString};

#[cfg(not(feature = "std"))]
pub use alloc::borrow::ToOwned;

#[cfg(feature = "std")]
pub use std::borrow::ToOwned;
