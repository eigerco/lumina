//! Serialize and deserialize any `T` that implements [`FromStr`]
//! and [`Display`] from or into string. Note this can be used for
//! all primitive data types.

use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::serializers::cow_str::CowStr;

/// Deserialize string into T
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    CowStr::deserialize(deserializer)?
        .parse::<T>()
        .map_err(serde::de::Error::custom)
}

/// Serialize from T into string
pub fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Display,
{
    value.to_string().serialize(serializer)
}
