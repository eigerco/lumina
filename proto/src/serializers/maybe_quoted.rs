//! A [`from_str`] serializer that additionally allows deserializing `T` using its own [`Deserialize`] impl.
//!
//! Binary serializers will only use the [`from_str`].
//!
//! [`from_str`]: crate::serializers::from_str

use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

use crate::serializers::cow_str::CowStr;

pub use crate::serializers::from_str::serialize;

/// Deserialize T directly or from string
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeQuoted<'a, T> {
        Direct(T),
        #[serde(borrow)]
        Quoted(CowStr<'a>),
    }

    if deserializer.is_human_readable() {
        match MaybeQuoted::deserialize(deserializer)? {
            MaybeQuoted::Direct(t) => Ok(t),
            MaybeQuoted::Quoted(s) => s.parse().map_err(serde::de::Error::custom),
        }
    } else {
        crate::serializers::from_str::deserialize(deserializer)
    }
}
