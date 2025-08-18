//! A [`from_str`] serializer that additionally allows deserializing `T` using its own [`Deserialize`] impl.
//!
//! [`from_str`]: crate::serializers::from_str

use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::serializers::cow_str::CowStr;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum MaybeQuoted<'a, T> {
    Direct(T),
    #[serde(borrow)]
    Quoted(CowStr<'a>),
}

#[derive(Serialize, Deserialize)]
enum MaybeQuotedTagged<T> {
    Direct(T),
    Quoted(String),
}

/// Serialize from T into string
pub fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + Display,
{
    if serializer.is_human_readable() {
        value.to_string().serialize(serializer)
    } else {
        MaybeQuotedTagged::<T>::Quoted(value.to_string()).serialize(serializer)
    }
}

/// Deserialize T directly or from string
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    if deserializer.is_human_readable() {
        match MaybeQuoted::deserialize(deserializer)? {
            MaybeQuoted::Direct(t) => Ok(t),
            MaybeQuoted::Quoted(s) => s.parse().map_err(serde::de::Error::custom),
        }
    } else {
        match MaybeQuotedTagged::deserialize(deserializer)? {
            MaybeQuotedTagged::Direct(t) => Ok(t),
            MaybeQuotedTagged::Quoted(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}
