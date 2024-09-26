use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Deserialize `null` as `Default`.
/// `#[serde(default)]` only works if value is missing entirely
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let value = Option::<T>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

/// Serialize value as `Some(value)`, provided for compatibility
/// with deserializer
pub fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    serializer.serialize_some(value)
}
