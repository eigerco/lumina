//! Custom [`serde`] serializer for [`tendermint::hash::Hash`].

use std::str::FromStr;

use celestia_proto::serializers::cow_str::CowStr;
use serde::{Deserialize, Deserializer, Serializer};
use tendermint::hash::Hash;

/// Deserialize [`tendermint::hash::Hash`].
pub fn deserialize<'de, D>(deserializer: D) -> Result<Hash, D::Error>
where
    D: Deserializer<'de>,
{
    let hex = Option::<CowStr>::deserialize(deserializer)?.unwrap_or_default();
    Hash::from_str(&hex).map_err(serde::de::Error::custom)
}

/// Serialize [`tendermint::hash::Hash`].
pub fn serialize<S>(value: &Hash, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_some(&value.to_string())
}
