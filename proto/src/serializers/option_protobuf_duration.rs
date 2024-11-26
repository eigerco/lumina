//! [`serde`] serializer for optional [`Duration`].

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tendermint_proto::google::protobuf::Duration;

#[derive(Serialize, Deserialize)]
struct Def {
    seconds: i64,
    nanos: i32,
}

/// Deserialize [`Option<Duration>`].
pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<Def>::deserialize(deserializer)?.map(|def| Duration {
            seconds: def.seconds,
            nanos: def.nanos,
        }),
    )
}

/// Serialize [`Option<Duration>`].
pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => {
            let def = Def {
                seconds: value.seconds,
                nanos: value.nanos,
            };
            serializer.serialize_some(&def)
        }
        None => serializer.serialize_none(),
    }
}
