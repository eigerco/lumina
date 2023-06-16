use base64::prelude::*;
use serde::de::{Deserialize, Deserializer, Error};
use serde::ser::Serializer;

pub(crate) fn serialize<T, S>(bytes: T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: AsRef<[u8]>,
    S: Serializer,
{
    let b64 = BASE64_STANDARD.encode(bytes.as_ref());
    serializer.serialize_str(&b64)
}

pub(crate) fn deserialize<'a, 'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: From<Vec<u8>>,
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let bytes = BASE64_STANDARD
        .decode(s)
        .map_err(|e| Error::custom(e.to_string()))?;
    Ok(bytes.into())
}
